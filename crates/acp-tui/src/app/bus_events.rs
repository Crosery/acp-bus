use tokio::sync::mpsc;

use acp_core::channel::{MessageKind, MessageStatus, MessageTransport, SystemKind};
use acp_core::client::{
    BusEvent, BusSendResult, CreateAgentResult, RemoveAgentResult, SendAndWaitResult,
};

use crate::i18n;

use super::{
    dispatch_group_sequential, do_prompt, do_prompt_with_reply, start_agent_bg, BusContext,
};
use super::prompting::is_main_instance;

pub(crate) async fn append_comm_log(
    channel: &std::sync::Arc<tokio::sync::Mutex<acp_core::channel::Channel>>,
    mut entry: acp_core::comm_log::CommLogEntry,
) {
    let (cwd, channel_id) = {
        let ch = channel.lock().await;
        (ch.cwd.clone(), ch.channel_id.clone())
    };
    entry.channel_id = channel_id;
    let _ = acp_core::comm_log::append(&cwd, &entry).await;
}

pub(crate) async fn handle_bus_event(
    ctx: &BusContext,
    event: BusEvent,
    bus_tx: &mpsc::UnboundedSender<BusEvent>,
) {
    match event {
        BusEvent::SendMessage {
            from_agent,
            to_agent,
            content,
            reply_tx,
        } => {
            let mut log_entry = acp_core::comm_log::entry("", "bus_send");
            log_entry.from = Some(from_agent.clone());
            log_entry.to = Some(to_agent.clone());
            log_entry.transport = Some("bus".to_string());
            log_entry.content = Some(content.clone());
            let result = {
                let mut ch = ctx.channel.lock().await;
                if from_agent == to_agent {
                    ch.post_audit(&format!(
                        "bus send rejected: {from_agent} cannot send to itself"
                    ));
                    BusSendResult {
                        message_id: None,
                        delivered: false,
                        error: Some("cannot send message to yourself".to_string()),
                    }
                } else if !ch.agents.contains_key(&to_agent) {
                    let _ = ch.post_message(
                        &from_agent,
                        Some(to_agent.clone()),
                        &content,
                        MessageKind::Chat,
                        MessageTransport::BusTool,
                        MessageStatus::Failed,
                        Some("target agent not found".to_string()),
                        true,
                    );
                    let id = ch.messages.last().map(|m| m.id);
                    ch.post_audit(&format!(
                        "bus send failed: {from_agent} -> {to_agent} (message #{})",
                        id.unwrap_or(0)
                    ));
                    log_entry.status = Some("failed".to_string());
                    log_entry.message_id = id;
                    log_entry.detail = Some("target agent not found".to_string());
                    BusSendResult {
                        message_id: id,
                        delivered: false,
                        error: Some("target agent not found".to_string()),
                    }
                } else {
                    let (conversation_id, reply_to) =
                        ch.resolve_reply_context(&from_agent, &to_agent);
                    let message_id = ch.post_directed_with_refs(
                        &from_agent,
                        &to_agent,
                        &content,
                        MessageKind::Chat,
                        MessageTransport::BusTool,
                        MessageStatus::Delivered,
                        conversation_id,
                        reply_to,
                    );
                    // Mark bus activity so "无文本输出" is suppressed
                    if let Some(agent) = ch.agents.get_mut(&from_agent) {
                        agent.has_bus_activity = true;
                    }
                    if reply_to.is_none() && ch.agents.contains_key(&from_agent) {
                        ch.mark_waiting(
                            &from_agent,
                            &to_agent,
                            conversation_id.unwrap_or(message_id),
                        );
                    } else if let Some(conv_id) = conversation_id {
                        ch.post_audit(&format!(
                            "conversation #{conv_id} closed: {from_agent} -> {to_agent}"
                        ));
                    }
                    ch.post_audit(&format!(
                        "bus send delivered: {from_agent} -> {to_agent} (message #{message_id})"
                    ));
                    log_entry.status = Some("delivered".to_string());
                    log_entry.message_id = Some(message_id);
                    log_entry.conversation_id = Some(conversation_id.unwrap_or(message_id));
                    log_entry.reply_to = reply_to;
                    log_entry.detail = Some(if reply_to.is_some() {
                        format!(
                            "reply closed conversation #{}",
                            conversation_id.unwrap_or(message_id)
                        )
                    } else {
                        format!(
                            "accepted by TUI dispatch; waiting for reply on #{}",
                            conversation_id.unwrap_or(message_id)
                        )
                    });
                    BusSendResult {
                        message_id: Some(message_id),
                        delivered: true,
                        error: None,
                    }
                }
            };
            append_comm_log(&ctx.channel, log_entry).await;
            if result.delivered {
                let ctx = ctx.clone();
                let prompt = format!("[Async message from {from_agent}]\n{content}");
                tokio::spawn(do_prompt(to_agent, prompt, ctx));
            }
            let _ = reply_tx.send(result);
        }
        BusEvent::ListAgents { reply_tx, .. } => {
            let agents = {
                let ch = ctx.channel.lock().await;
                let now = chrono::Utc::now().timestamp();
                ch.agents
                    .iter()
                    .map(|(name, agent)| acp_core::client::AgentInfo {
                        name: name.clone(),
                        status: agent.status.to_string(),
                        adapter: agent.adapter_name.clone(),
                        activity: agent.activity.clone(),
                        active_secs: agent.prompt_start_time.map(|t| (now - t).max(0)),
                        current_task: agent.current_task.clone(),
                        inbox_depth: 0, // Will be populated when inbox is implemented
                        waiting_for: agent.waiting_reply_from.clone(),
                    })
                    .collect()
            };
            let _ = reply_tx.send(agents);
        }
        BusEvent::CreateAgent {
            from_agent,
            name,
            adapter,
            task,
            reply_tx,
        } => {
            let result = if !is_main_instance(&from_agent) {
                CreateAgentResult {
                    ok: false,
                    error: Some("only main agent can create agents".into()),
                }
            } else {
                // Track this agent for completion monitoring
                {
                    let mut pt = ctx.pending_tasks.lock().await;
                    pt.track(&name);
                }
                // Spawn agent in background
                let ctx = ctx.clone();
                let bus_tx = Some(bus_tx.clone());
                let task_clone = task.clone();
                let name_clone = name.clone();
                tokio::spawn(async move {
                    let ctx2 = ctx.clone();
                    start_agent_bg(name_clone.clone(), adapter, ctx, bus_tx).await;
                    // If task was provided, dispatch it after agent connects
                    if let Some(task_content) = task_clone {
                        do_prompt(name_clone, task_content, ctx2).await;
                    }
                });
                CreateAgentResult {
                    ok: true,
                    error: None,
                }
            };
            let _ = reply_tx.send(result);
        }
        BusEvent::RemoveAgent {
            from_agent,
            name,
            reply_tx,
        } => {
            let result = if !is_main_instance(&from_agent) {
                RemoveAgentResult {
                    ok: false,
                    error: Some("only main agent can remove agents".into()),
                }
            } else if name == "main" {
                RemoveAgentResult {
                    ok: false,
                    error: Some("cannot remove main agent".into()),
                }
            } else {
                {
                    let mut map = ctx.clients.lock().await;
                    if let Some(client) = map.remove(&name) {
                        let mut c = client.lock().await;
                        c.stop().await;
                    }
                }
                let mut ch = ctx.channel.lock().await;
                ch.remove_agent(&name);
                RemoveAgentResult {
                    ok: true,
                    error: None,
                }
            };
            let _ = reply_tx.send(result);
        }
        BusEvent::SendAndWait {
            from_agent,
            to_agent,
            content,
            timeout_secs,
            reply_tx,
        } => {
            // 1. Deadlock detection
            {
                let mut wg = ctx.wait_graph.lock().await;
                if let Err(e) = wg.add_wait(&from_agent, &to_agent) {
                    let _ = reply_tx.send(SendAndWaitResult {
                        ok: false,
                        reply_content: None,
                        from_agent: None,
                        error: Some(format!("deadlock detected: {e}")),
                    });
                    return;
                }
            }

            // 2. Validate target agent exists
            {
                let ch = ctx.channel.lock().await;
                if !ch.agents.contains_key(&to_agent) {
                    // Remove the wait edge we just added
                    let mut wg = ctx.wait_graph.lock().await;
                    wg.remove_wait(&from_agent);
                    let _ = reply_tx.send(SendAndWaitResult {
                        ok: false,
                        reply_content: None,
                        from_agent: None,
                        error: Some(format!("target agent '{to_agent}' not found")),
                    });
                    return;
                }
            }

            // 3. Post message to channel (visible in UI)
            {
                let mut ch = ctx.channel.lock().await;
                let (conversation_id, reply_to) = ch.resolve_reply_context(&from_agent, &to_agent);
                ch.post_directed_with_refs(
                    &from_agent,
                    &to_agent,
                    &content,
                    MessageKind::Chat,
                    MessageTransport::BusTool,
                    MessageStatus::Delivered,
                    conversation_id,
                    reply_to,
                );
            }

            // 4. Store pending wait (keyed by the waiting agent)
            {
                let mut pw = ctx.pending_waits.lock().await;
                pw.insert(from_agent.clone(), reply_tx);
            }

            // 5. Dispatch prompt to target agent
            let ctx_clone = ctx.clone();
            let to = to_agent.clone();
            let from = from_agent.clone();
            let prompt = format!(
                "[{from_agent} is waiting for your reply (timeout {timeout_secs}s). Reply directly to {from_agent} then STOP. If you need other agents, reply first, then send async messages.]\n{content}"
            );
            tokio::spawn(do_prompt_with_reply(to, prompt, ctx_clone, from.clone()));

            // 6. Timeout cleanup (independent task)
            let pw = ctx.pending_waits.clone();
            let wg = ctx.wait_graph.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_secs(timeout_secs)).await;
                let mut pending = pw.lock().await;
                if let Some(tx) = pending.remove(&from) {
                    let _ = tx.send(SendAndWaitResult {
                        ok: false,
                        reply_content: None,
                        from_agent: None,
                        error: Some("timeout: no reply received".into()),
                    });
                }
                let mut graph = wg.lock().await;
                graph.remove_wait(&from);
            });
        }
        BusEvent::Reply {
            from_agent,
            to_agent,
            content,
            in_reply_to,
            reply_tx,
        } => {
            // 1. Post reply message to channel (visible in UI)
            let message_id = {
                let mut ch = ctx.channel.lock().await;
                let id = ch.post_directed_with_refs(
                    &from_agent,
                    &to_agent,
                    &content,
                    MessageKind::Chat,
                    MessageTransport::BusTool,
                    MessageStatus::Delivered,
                    None,
                    in_reply_to,
                );
                // Mark bus activity
                if let Some(agent) = ch.agents.get_mut(&from_agent) {
                    agent.has_bus_activity = true;
                }
                id
            };

            // 2. Fulfill pending wait if the target agent was waiting
            let fulfilled = {
                let mut pw = ctx.pending_waits.lock().await;
                if let Some(tx) = pw.remove(&to_agent) {
                    let _ = tx.send(SendAndWaitResult {
                        ok: true,
                        reply_content: Some(content.clone()),
                        from_agent: Some(from_agent.clone()),
                        error: None,
                    });
                    true
                } else {
                    false
                }
            };

            // 3. Clean up wait graph
            if fulfilled {
                let mut wg = ctx.wait_graph.lock().await;
                wg.remove_wait(&to_agent);
            }

            let _ = reply_tx.send(BusSendResult {
                message_id: Some(message_id),
                delivered: true,
                error: None,
            });
        }
        BusEvent::CreateGroup {
            from_agent,
            name,
            members,
            reply_tx,
        } => {
            let mut ch = ctx.channel.lock().await;
            if ch.create_group(&name, &from_agent) {
                for member in &members {
                    if let Some(group) = ch.groups.get_mut(&name) {
                        group.add_member(member);
                    }
                }
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: true,
                    error: None,
                });
            } else {
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: false,
                    error: Some(i18n::err_group_exists(&name)),
                });
            }
        }
        BusEvent::GroupMessage {
            from_agent,
            group_name,
            content,
            rounds,
            reply_tx,
        } => {
            let recipients = {
                let ch = ctx.channel.lock().await;
                ch.group_recipients(&group_name, &from_agent)
            };
            if recipients.is_empty() {
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: false,
                    error: Some(i18n::ERR_GROUP_NO_MEMBERS.into()),
                });
                return;
            }
            // Post ONE group message visible to all members + collect history
            let history = {
                let mut ch = ctx.channel.lock().await;
                ch.post_group(&group_name, &from_agent, &content);
                if let Some(agent) = ch.agents.get_mut(&from_agent) {
                    agent.has_bus_activity = true;
                }
                ch.group_history(&group_name, 10)
            };
            // Sequential dispatch: each agent sees accumulated context
            let ctx = ctx.clone();
            let gn = group_name.clone();
            let fa = from_agent.clone();
            let ct = content.clone();
            tokio::spawn(dispatch_group_sequential(
                recipients, history, gn, fa, ct, ctx, rounds,
            ));
            let _ = reply_tx.send(BusSendResult {
                message_id: None,
                delivered: true,
                error: None,
            });
        }
        BusEvent::GroupAdd {
            from_agent: _,
            group_name,
            member,
            reply_tx,
        } => {
            let mut ch = ctx.channel.lock().await;
            let agent_exists = ch.agents.contains_key(&member);
            let group_exists = ch.groups.contains_key(&group_name);
            if !group_exists {
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: false,
                    error: Some(i18n::err_group_not_found(&group_name)),
                });
            } else if !agent_exists {
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: false,
                    error: Some(i18n::err_agent_not_found(&member)),
                });
            } else {
                let added = ch.groups.get_mut(&group_name).unwrap().add_member(&member);
                if added {
                    ch.post_system_typed(
                        &i18n::sys_member_joined_group(&member, &group_name),
                        SystemKind::General,
                    );
                }
                let _ = reply_tx.send(BusSendResult {
                    message_id: None,
                    delivered: true,
                    error: if added {
                        None
                    } else {
                        Some(i18n::err_member_already_in_group(&member))
                    },
                });
            }
        }
    }
}
