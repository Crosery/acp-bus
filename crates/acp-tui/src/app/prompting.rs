use tokio::sync::mpsc;

use acp_core::agent::AgentStatus;
use acp_core::channel::{MessageKind, MessageStatus, MessageTransport, SystemKind};
use acp_core::client::{BusEvent, SendAndWaitResult};
use acp_core::router;

use crate::i18n;

use super::{start_agent_bg, BusContext, ClientMap};

/// Maximum number of elastic main instances (main + main-2..main-N).
const MAX_MAIN_INSTANCES: usize = 5;

/// Check if an agent name is a main instance (main, main-2, main-3, ...).
pub(crate) fn is_main_instance(name: &str) -> bool {
    name == "main"
        || name
            .strip_prefix("main-")
            .is_some_and(|s| s.parse::<u32>().is_ok())
}

/// Execute a prompt to an agent and post the reply back to the channel
pub(crate) fn do_prompt(
    name: String,
    content: String,
    ctx: BusContext,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, ctx, None, None, false, None))
}

pub(crate) fn do_prompt_with_reply(
    name: String,
    content: String,
    ctx: BusContext,
    reply_to: String,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, ctx, Some(reply_to), None, false, None))
}

/// Like do_prompt but with an optional image attachment.
pub(crate) async fn do_prompt_with_image(
    name: String,
    content: String,
    ctx: BusContext,
    image: Option<super::PendingImage>,
) {
    do_prompt_inner(name, content, ctx, None, None, false, image).await;
}

/// Like do_prompt but skips the scheduler gate (already drained from queue).
fn do_prompt_scheduled(
    name: String,
    content: String,
    ctx: BusContext,
    reply_to: Option<String>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, ctx, reply_to, None, true, None))
}

/// Build a group prompt with conversation history context.
fn format_group_prompt(
    group_name: &str,
    from: &str,
    content: &str,
    history: &[(String, String)],
) -> String {
    let mut prompt = String::new();
    if !history.is_empty() {
        prompt.push_str(&format!("[Group '{group_name}' — conversation so far]\n"));
        for (speaker, msg) in history {
            // Truncate long messages in history to save tokens
            let truncated: String = msg.chars().take(500).collect();
            let suffix = if msg.chars().count() > 500 { "..." } else { "" };
            prompt.push_str(&format!("{speaker}: {truncated}{suffix}\n"));
        }
        prompt.push_str(&format!(
            "\n[New message from {from} — respond to this]\n{content}"
        ));
    } else {
        prompt.push_str(&format!(
            "[Group '{group_name}' message from {from}]\n{content}"
        ));
    }
    prompt
}

/// Dispatch group prompts sequentially so each agent sees prior responses.
/// After all rounds complete, marks group members as done in PendingTasks
/// and notifies main if all dispatched agents have finished.
pub(crate) async fn dispatch_group_sequential(
    recipients: Vec<String>,
    mut history: Vec<(String, String)>,
    group_name: String,
    from_agent: String,
    content: String,
    ctx: BusContext,
    rounds: u32,
) {
    let rounds = rounds.max(1).min(10); // clamp to [1, 10]
    for round in 0..rounds {
        for recipient in &recipients {
            // First round uses the original message; subsequent rounds use a continuation prompt
            let (effective_from, effective_content) = if round == 0 {
                (from_agent.clone(), content.clone())
            } else {
                // For continuation rounds, prompt agents to continue the discussion
                let cont = format!(
                    "[Group '{group_name}' — round {}/{rounds}] The discussion continues. \
                     Review the conversation so far and add your next argument or response. \
                     Be concise (3-4 sentences).",
                    round + 1,
                );
                ("system".to_string(), cont)
            };
            let prompt =
                format_group_prompt(&group_name, &effective_from, &effective_content, &history);
            do_prompt_inner(
                recipient.clone(),
                prompt,
                ctx.clone(),
                None,
                Some(group_name.clone()),
                false,
                None,
            )
            .await;
            // After agent completes, collect its reply from the channel to add to history
            let reply = {
                let ch = ctx.channel.lock().await;
                ch.group_history(&group_name, 1)
                    .last()
                    .filter(|(speaker, _)| speaker == recipient)
                    .map(|(_, msg)| msg.clone())
            };
            if let Some(reply_text) = reply {
                history.push((recipient.clone(), reply_text));
            }
        }
    }

    // All rounds complete — mark group members as done in PendingTasks.
    // complete() returns true when ALL tracked agents (including non-group ones) are done.
    let all_done = {
        let mut pt = ctx.pending_tasks.lock().await;
        let mut result = false;
        for r in &recipients {
            result = pt.complete(r);
        }
        result
    };
    if all_done {
        let ctx2 = ctx.clone();
        tokio::spawn(do_prompt(
            "main".to_string(),
            format!(
                "[System: Group '{group_name}' discussion completed ({rounds} round(s), {} participants). \
                 Review the group history and deliver a summary to the user.]",
                recipients.len()
            ),
            ctx2,
        ));
    }
}

async fn do_prompt_inner(
    mut name: String,
    content: String,
    ctx: BusContext,
    reply_to: Option<String>,
    group: Option<String>,
    scheduled: bool,
    image: Option<super::PendingImage>,
) {
    // Elastic main gate: route to an idle main instance, or spawn a new one.
    // Skip when scheduled=true (already drained from queue, avoid re-entry).
    if name == "main" && !scheduled {
        use acp_core::fair_scheduler::Priority;

        // Single lock to atomically find idle instance OR reserve a new slot.
        // This prevents race conditions where multiple concurrent calls
        // all try to create the same main-N.
        enum MainRoute {
            Idle(String),
            SpawnNew(String, String), // (new_name, adapter)
            Queue,
        }

        let route = {
            let mut ch = ctx.channel.lock().await;
            // 1. Find an idle main instance
            let idle = if ch
                .agents
                .get("main")
                .is_some_and(|a| a.status == AgentStatus::Idle)
            {
                Some("main".to_string())
            } else {
                (2..=MAX_MAIN_INSTANCES)
                    .map(|i| format!("main-{i}"))
                    .find(|n| {
                        ch.agents
                            .get(n)
                            .is_some_and(|a| a.status == AgentStatus::Idle)
                    })
            };

            if let Some(target) = idle {
                MainRoute::Idle(target)
            } else {
                // 2. Count existing instances
                let instance_count = 1
                    + (2..=MAX_MAIN_INSTANCES)
                        .filter(|i| ch.agents.contains_key(&format!("main-{i}")))
                        .count();
                if instance_count < MAX_MAIN_INSTANCES {
                    let new_name = format!("main-{}", instance_count + 1);
                    let adapter = ch
                        .agents
                        .get("main")
                        .map(|a| a.adapter_name.clone())
                        .unwrap_or_else(|| "claude".to_string());
                    // Reserve the slot NOW (inside the lock) to prevent duplicates
                    let agent = acp_core::agent::Agent::new_spawned(
                        new_name.clone(),
                        adapter.clone(),
                        None,
                    );
                    ch.agents.insert(new_name.clone(), agent);
                    MainRoute::SpawnNew(new_name, adapter)
                } else {
                    MainRoute::Queue
                }
            }
        };

        match route {
            MainRoute::Idle(target) => {
                name = target;
                let mut sched = ctx.scheduler.lock().await;
                let priority = if reply_to.is_none() {
                    Priority::High
                } else {
                    Priority::Normal
                };
                match sched.enqueue_with_reply(&content, None, reply_to.as_deref(), priority) {
                    Ok(_) => {}
                    Err(msg) => {
                        let mut ch = ctx.channel.lock().await;
                        ch.post(i18n::SYS_SENDER_SYSTEM, &msg, true);
                        return;
                    }
                }
            }
            MainRoute::SpawnNew(new_name, adapter) => {
                // Slot already reserved in agents map — now start the process
                let ctx2 = ctx.clone();
                super::lifecycle::start_agent(
                    new_name.clone(),
                    adapter,
                    true, // is_main — gets main system prompt
                    ctx2,
                    None,
                )
                .await;
                name = new_name;
            }
            MainRoute::Queue => {
                let should_send = {
                    let mut sched = ctx.scheduler.lock().await;
                    let priority = if reply_to.is_none() {
                        Priority::High
                    } else {
                        Priority::Normal
                    };
                    match sched.enqueue_with_reply(
                        &content,
                        None,
                        reply_to.as_deref(),
                        priority,
                    ) {
                        Ok(immediate) => immediate,
                        Err(msg) => {
                            let mut ch = ctx.channel.lock().await;
                            ch.post(i18n::SYS_SENDER_SYSTEM, &msg, true);
                            return;
                        }
                    }
                };
                if !should_send {
                    let depth = {
                        let sched = ctx.scheduler.lock().await;
                        sched.queue_depth()
                    };
                    let mut ch = ctx.channel.lock().await;
                    ch.post_system_typed(
                        &i18n::sys_all_main_busy(MAX_MAIN_INSTANCES, depth),
                        SystemKind::QueueNotice,
                    );
                    return;
                }
            }
        }
    }
    // Wait for agent entry to appear in channel (may be created by a concurrent spawn)
    let agent_found = {
        let mut found = false;
        for _ in 0..20 {
            let ch = ctx.channel.lock().await;
            if ch.agents.contains_key(&name) {
                found = true;
                break;
            }
            drop(ch);
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        found
    };
    if !agent_found {
        drain_scheduler_if_main(&name, &ctx).await;
        return;
    }

    // Build payload (system prompt is injected via ACP _meta at session creation)
    let payload = {
        let mut ch = ctx.channel.lock().await;
        let agent = ch.agents.get_mut(&name).unwrap();
        agent.status = AgentStatus::Streaming;
        agent.streaming = true;
        agent.stream_buf.clear();
        agent.thinking_buf.clear();
        agent.has_bus_activity = false;
        agent.activity = Some("receiving".into());
        agent.current_task = Some(content.chars().take(100).collect());
        // First prompt: inject identity reminder so agent knows who it is
        let text = if !agent.prompted {
            format!(
                "[Identity: You are {name}, an agent in the acp-bus channel. Your adapter is {}.]\n\n{content}",
                agent.adapter_name
            )
        } else {
            content.clone()
        };
        agent.prompted = true;
        agent.prompt_start_time = Some(chrono::Utc::now().timestamp());
        ch.state_changed();
        text
    };

    // Get client handle — wait if agent is still connecting
    let client = {
        let mut client = None;
        for _ in 0..60 {
            let map = ctx.clients.lock().await;
            if let Some(c) = map.get(&name) {
                client = Some(c.clone());
                break;
            }
            drop(map);
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        client
    };
    let client = match client {
        Some(c) => c,
        None => {
            let mut ch = ctx.channel.lock().await;
            ch.post(
                i18n::SYS_SENDER_SYSTEM,
                &i18n::sys_agent_not_connected(&name),
                true,
            );
            if let Some(agent) = ch.agents.get_mut(&name) {
                agent.status = AgentStatus::Idle;
                agent.streaming = false;
                agent.prompt_start_time = None;
                agent.pending_task = Some(content);
            }
            ch.state_changed();
            drain_scheduler_if_main(&name, &ctx).await;
            return;
        }
    };

    // Dispatch messages are posted by the caller (handle_input or do_prompt_inner routing).
    // No need to post again here.

    // If image is attached, save to temp file and prepend path to prompt text.
    // The file is cleaned up after the prompt completes.
    let mut paste_file: Option<std::path::PathBuf> = None;
    let final_payload = if let Some(img) = image {
        let cwd = {
            let ch = ctx.channel.lock().await;
            ch.cwd.clone()
        };
        let (filepath, prompt) =
            super::image::save_paste_image(&img, &cwd, &payload).await;
        paste_file = Some(filepath);
        prompt
    } else {
        payload.clone()
    };

    // Execute prompt
    let stop_reason = {
        let c = client.lock().await;
        c.prompt(&final_payload).await
    };

    // Collect reply from stream_buf
    let reply = {
        let mut ch = ctx.channel.lock().await;
        let buf = if let Some(agent) = ch.agents.get_mut(&name) {
            agent.streaming = false;
            agent.status = AgentStatus::Idle;
            agent.activity = None;
            agent.prompt_start_time = None;
            agent.current_task = None;
            agent.thinking_buf.clear();
            agent.waiting_reply_from = None;
            agent.waiting_since = None;
            agent.waiting_conversation_id = None;
            std::mem::take(&mut agent.stream_buf)
        } else {
            return;
        };
        ch.state_changed();
        buf
    };

    // Check if agent already communicated via bus tools (bus_reply etc.)
    let suppress_auto_reply = {
        let ch = ctx.channel.lock().await;
        ch.agents
            .get(&name)
            .is_some_and(|a| a.should_suppress_auto_reply())
    };

    match stop_reason {
        Ok(_) => {
            if !reply.is_empty() {
                // Parse and execute /add commands from agent output
                let added_agents = execute_agent_commands(&reply, &ctx, None).await;

                // Group context: ALWAYS post text output to group, even if agent
                // used bus tools (bus_reply etc.) — the group needs to see the response
                if let Some(ref gname) = group {
                    let mut ch = ctx.channel.lock().await;
                    ch.post_group(gname, &name, &reply);
                } else {
                    // Check if reply has @mentions that need routing
                    let known_agents = {
                        let ch = ctx.channel.lock().await;
                        ch.agents.keys().cloned().collect::<Vec<_>>()
                    };
                    let targets = router::route(&reply, &name, &known_agents, 1);

                    if targets.is_empty() {
                        if suppress_auto_reply {
                            // Agent already replied via bus tools — skip auto-reply
                            // to prevent duplicate messages
                        } else if let Some(ref sender) = reply_to {
                            // Auto-route reply back to the agent who sent the bus message
                            {
                                let mut ch = ctx.channel.lock().await;
                                let (conversation_id, reply_ref) =
                                    ch.resolve_reply_context(&name, sender);
                                ch.post_directed_with_refs(
                                    &name,
                                    sender,
                                    &reply,
                                    MessageKind::Chat,
                                    MessageTransport::BusTool,
                                    MessageStatus::Delivered,
                                    conversation_id,
                                    reply_ref,
                                );
                                if let Some(conv_id) = conversation_id {
                                    ch.post_audit(&format!(
                                        "auto-reply #{conv_id}: {name} -> {sender}"
                                    ));
                                }
                            }
                            // Check if sender has a pending send_and_wait — fulfill it
                            // instead of spawning a new prompt (which can't be processed
                            // while the sender is blocked on the wait).
                            let fulfilled = {
                                let mut pw = ctx.pending_waits.lock().await;
                                if let Some(tx) = pw.remove(sender) {
                                    let _ = tx.send(SendAndWaitResult {
                                        ok: true,
                                        reply_content: Some(reply.clone()),
                                        from_agent: Some(name.clone()),
                                        error: None,
                                    });
                                    true
                                } else {
                                    false
                                }
                            };
                            if fulfilled {
                                let mut wg = ctx.wait_graph.lock().await;
                                wg.remove_wait(sender);
                            } else {
                                // No pending wait — prompt the sender with the reply
                                let ctx2 = ctx.clone();
                                let sender = sender.clone();
                                let reply_content = format!("[Reply from {name}]\n{reply}");
                                tokio::spawn(do_prompt(sender, reply_content, ctx2));
                            }
                        } else if reply_to.is_none() {
                            // Post reply as broadcast
                            let mut ch = ctx.channel.lock().await;
                            ch.post(&name, &reply, true);
                        }
                        // If reply_to is Some but no @mentions: this agent was replying
                        // to a send_and_wait. Any remaining text after bus_reply is
                        // just confirmation noise — suppress it.
                    } else {
                        // Has @mentions — skip broadcast to avoid duplicate messages.
                        // Only post directed messages to each target below.

                        // Wait for newly added agents
                        if !added_agents.is_empty() {
                            wait_for_agents(&added_agents, &ctx.clients, 30).await;
                        }

                        // Post per-agent segments and dispatch
                        for target in targets {
                            let tname = target.name.clone();
                            let tcontent = target.content.clone();

                            // Post dispatch message visible in agent's tab
                            {
                                let mut ch = ctx.channel.lock().await;
                                let (conversation_id, reply_to) =
                                    ch.resolve_reply_context(&name, &tname);
                                let message_id = ch.post_directed_with_refs(
                                    &name,
                                    &tname,
                                    &tcontent,
                                    MessageKind::Task,
                                    MessageTransport::MentionRoute,
                                    MessageStatus::Delivered,
                                    conversation_id,
                                    reply_to,
                                );
                                if reply_to.is_none() && ch.agents.contains_key(&name) {
                                    ch.mark_waiting(
                                        &name,
                                        &tname,
                                        conversation_id.unwrap_or(message_id),
                                    );
                                } else if let Some(conv_id) = conversation_id {
                                    ch.post_audit(&format!(
                                        "conversation #{conv_id} closed: {name} -> {tname}"
                                    ));
                                }
                                let mut entry =
                                    acp_core::comm_log::entry(&ch.channel_id, "agent_dispatch");
                                entry.from = Some(name.clone());
                                entry.to = Some(tname.clone());
                                entry.transport = Some("mention".to_string());
                                entry.status = Some("delivered".to_string());
                                entry.message_id = Some(message_id);
                                entry.conversation_id = Some(conversation_id.unwrap_or(message_id));
                                entry.reply_to = reply_to;
                                entry.content = Some(tcontent.clone());
                                entry.detail = Some(if reply_to.is_some() {
                                    format!(
                                        "reply closed conversation #{}",
                                        conversation_id.unwrap_or(message_id)
                                    )
                                } else {
                                    format!(
                                        "agent routed task via @mention; waiting for reply on #{}",
                                        conversation_id.unwrap_or(message_id)
                                    )
                                });
                                let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
                            }

                            // If this target has a pending send_and_wait, fulfill it
                            // instead of dispatching a new prompt (which would block
                            // since the target's client is waiting for the reply).
                            if let Some(ref sender) = reply_to {
                                if &tname == sender {
                                    let fulfilled = {
                                        let mut pw = ctx.pending_waits.lock().await;
                                        if let Some(tx) = pw.remove(sender) {
                                            let _ = tx.send(SendAndWaitResult {
                                                ok: true,
                                                reply_content: Some(tcontent.clone()),
                                                from_agent: Some(name.clone()),
                                                error: None,
                                            });
                                            true
                                        } else {
                                            false
                                        }
                                    };
                                    if fulfilled {
                                        let mut wg = ctx.wait_graph.lock().await;
                                        wg.remove_wait(sender);
                                        continue;
                                    }
                                }
                            }

                            let ctx2 = ctx.clone();
                            let prompt_with_sender = format!("[Message from {name}]\n{tcontent}");
                            tokio::spawn(do_prompt(tname, prompt_with_sender, ctx2));
                        }
                    }
                } // end else (non-group routing)
            }
            {
                let mut ch = ctx.channel.lock().await;
                ch.post_system_typed(&i18n::sys_agent_complete(&name), SystemKind::AgentComplete);
                // Clear waiting state of any agent that was waiting for this agent
                let waiters: Vec<String> = ch
                    .agents
                    .iter()
                    .filter(|(_, a)| a.waiting_reply_from.as_deref() == Some(&name))
                    .map(|(n, _)| n.clone())
                    .collect();
                for waiter in waiters {
                    if let Some(a) = ch.agents.get_mut(&waiter) {
                        a.waiting_reply_from = None;
                        a.waiting_since = None;
                        a.waiting_conversation_id = None;
                    }
                }
            }
            // Mark this agent as completed in PendingTasks and notify main if all done.
            // Skip for group prompts — agents in group discussions are prompted
            // multiple times per round, so completing on each would fire prematurely.
            if !is_main_instance(&name) && group.is_none() {
                let all_done = {
                    let mut pt = ctx.pending_tasks.lock().await;
                    pt.complete(&name)
                };
                if all_done {
                    let ctx2 = ctx.clone();
                    tokio::spawn(do_prompt(
                        "main".to_string(),
                        "[System: All dispatched agents have completed their tasks. Check group histories and agent results, then deliver a summary to the user.]".to_string(),
                        ctx2,
                    ));
                }
            }
        }
        Err(e) => {
            let mut ch = ctx.channel.lock().await;
            ch.post_system_typed(&i18n::sys_agent_error(&name, &e.to_string()), SystemKind::AgentError);
            if let Some(agent) = ch.agents.get_mut(&name) {
                agent.status = AgentStatus::Error;
            }
            // Clear waiting state of agents waiting for this one
            let waiters: Vec<String> = ch
                .agents
                .iter()
                .filter(|(_, a)| a.waiting_reply_from.as_deref() == Some(&name))
                .map(|(n, _)| n.clone())
                .collect();
            for waiter in waiters {
                if let Some(a) = ch.agents.get_mut(&waiter) {
                    a.waiting_reply_from = None;
                    a.waiting_since = None;
                    a.waiting_conversation_id = None;
                }
            }
            ch.state_changed();
            // Error path: mark completed so PendingTasks doesn't block forever
            // Skip for group prompts (same rationale as Ok path)
            if !is_main_instance(&name) && group.is_none() {
                drop(ch); // release channel lock before acquiring pending_tasks lock
                let all_done = {
                    let mut pt = ctx.pending_tasks.lock().await;
                    pt.complete(&name)
                };
                if all_done {
                    let ctx2 = ctx.clone();
                    tokio::spawn(do_prompt(
                        "main".to_string(),
                        "[System: All dispatched agents have completed their tasks. Check group histories and agent results, then deliver a summary to the user.]".to_string(),
                        ctx2,
                    ));
                }
            }
        }
    }

    // Clean up temp paste file after prompt completes
    if let Some(ref path) = paste_file {
        super::image::cleanup_paste_file(path).await;
    }

    drain_scheduler_if_main(&name, &ctx).await;
}

/// Drain the scheduler queue for the main agent.
/// MUST be called on ALL exit paths from do_prompt_inner when name == "main",
/// otherwise the scheduler stays stuck in busy=true and main stops processing.
async fn drain_scheduler_if_main(name: &str, ctx: &BusContext) {
    // All main instances can drain the scheduler (they share the queue)
    if !is_main_instance(name) {
        return;
    }
    let next = {
        let mut sched = ctx.scheduler.lock().await;
        sched.drain()
    };
    if let Some(queued) = next {
        let ctx2 = ctx.clone();
        tokio::spawn(do_prompt_scheduled(
            "main".to_string(),
            queued.content,
            ctx2,
            queued.reply_to,
        ));
    }
}

/// Scan agent output for `/add name adapter` commands and execute them.
/// Returns names of newly added agents.
async fn execute_agent_commands(
    reply: &str,
    ctx: &BusContext,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
) -> Vec<String> {
    // Pre-parse: group multi-line /add commands (continuation lines don't start with / or @)
    let mut commands: Vec<String> = Vec::new();
    for line in reply.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('/') || trimmed.starts_with('@') {
            commands.push(trimmed.to_string());
        } else if !trimmed.is_empty() {
            // Continuation of previous command
            if let Some(last) = commands.last_mut() {
                last.push('\n');
                last.push_str(trimmed);
            }
        }
    }

    let mut added = Vec::new();
    for cmd_line in &commands {
        let trimmed = cmd_line.as_str();
        if trimmed.starts_with("/add ") {
            // Split only the first line for name/adapter, rest is task
            let first_line = trimmed.lines().next().unwrap_or(trimmed);
            let parts: Vec<&str> = first_line.splitn(4, ' ').collect();
            if parts.len() >= 3 {
                let agent_name = parts[1].to_string();
                let adapter_name = parts[2].to_string();
                // Task = remainder of first line + all continuation lines
                let first_line_task = if parts.len() >= 4 { parts[3] } else { "" };
                let continuation: String = trimmed.lines().skip(1).collect::<Vec<_>>().join("\n");
                let full_task = if continuation.is_empty() {
                    first_line_task.to_string()
                } else if first_line_task.is_empty() {
                    continuation
                } else {
                    format!("{first_line_task}\n{continuation}")
                };
                let task = if full_task.is_empty() {
                    None
                } else {
                    Some(full_task)
                };
                let exists = {
                    let ch = ctx.channel.lock().await;
                    ch.agents.contains_key(&agent_name)
                };
                if !exists {
                    start_agent_bg(
                        agent_name.clone(),
                        adapter_name,
                        ctx.clone(),
                        bus_tx.clone(),
                    )
                    .await;
                    added.push(agent_name.clone());

                    if let Some(task) = task {
                        let ctx2 = ctx.clone();
                        tokio::spawn(async move {
                            wait_for_agents(std::slice::from_ref(&agent_name), &ctx2.clients, 30)
                                .await;
                            {
                                let mut chan = ctx2.channel.lock().await;
                                chan.post_directed(
                                    "main",
                                    &agent_name,
                                    &task,
                                    MessageKind::Task,
                                    MessageTransport::MentionRoute,
                                    MessageStatus::Delivered,
                                );
                            }
                            do_prompt(agent_name, task, ctx2).await;
                        });
                    }
                }
            }
        } else if trimmed.starts_with("/remove ") {
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                let agent_name = parts[1].trim();
                if agent_name != "main" {
                    let mut map = ctx.clients.lock().await;
                    if let Some(client) = map.remove(agent_name) {
                        let mut c = client.lock().await;
                        c.stop().await;
                    }
                    drop(map);
                    let mut ch = ctx.channel.lock().await;
                    ch.remove_agent(agent_name);
                }
            }
        }
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_main_instance_matches() {
        assert!(is_main_instance("main"));
        assert!(is_main_instance("main-2"));
        assert!(is_main_instance("main-5"));
        assert!(is_main_instance("main-10"));
        assert!(!is_main_instance("main-b"));
        assert!(!is_main_instance("main-worker"));
        assert!(!is_main_instance("worker"));
        assert!(!is_main_instance("main-"));
    }
}

/// Wait for agents to get their client handles (i.e. finish handshake).
async fn wait_for_agents(names: &[String], clients: &ClientMap, timeout_secs: u64) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    loop {
        let all_ready = {
            let map = clients.lock().await;
            names.iter().all(|n| map.contains_key(n))
        };
        if all_ready {
            break;
        }
        if std::time::Instant::now() > deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
