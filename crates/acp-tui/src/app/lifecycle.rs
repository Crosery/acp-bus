use std::sync::Arc;

use tokio::sync::mpsc;

use acp_core::adapter::{self, AdapterOpts};
use acp_core::agent::{Agent, AgentStatus};
use acp_core::channel::{Channel, SystemKind};
use acp_core::client::{AcpClient, BusEvent, ClientEvent, SendAndWaitResult};

use crate::i18n;

use super::{do_prompt, BusContext, ClientMap, PendingWaits, SharedWaitGraph};

/// Start an agent process with full event listener.
///
/// When `is_main` is true, updates the existing "main" agent entry instead of
/// inserting a new one, and records comm-log lifecycle events.
pub(crate) async fn start_agent(
    name: String,
    adapter_name: String,
    is_main: bool,
    ctx: BusContext,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
) {
    let cwd = {
        let ch = ctx.channel.lock().await;
        ch.cwd.clone()
    };

    tokio::spawn(async move {
        let opts = AdapterOpts {
            bus_mode: true,
            is_main,
            agent_name: Some(name.clone()),
            channel_id: {
                let ch = ctx.channel.lock().await;
                Some(ch.channel_id.clone())
            },
            cwd: Some(cwd.clone()),
        };

        let mut config = match adapter::get(&adapter_name, &opts) {
            Ok(c) => c,
            Err(e) => {
                let mut ch = ctx.channel.lock().await;
                ch.post_system_typed(&i18n::sys_adapter_error(&e.to_string()), SystemKind::AgentError);
                return;
            }
        };
        config.socket_path = ctx.socket_path.clone();
        config.mcp_command = ctx.mcp_command.clone();

        let system_prompt = config.system_prompt.clone();

        {
            let mut ch = ctx.channel.lock().await;
            if let Some(agent) = ch.agents.get_mut(&name) {
                // Agent entry already exists (e.g. "main" created at startup) — update it
                agent.adapter_name = adapter_name.clone();
                agent.status = AgentStatus::Connecting;
            } else {
                // New agent — insert entry (works for workers AND elastic main-N)
                let agent = Agent::new_spawned(name.clone(), adapter_name.clone(), system_prompt);
                ch.agents.insert(name.clone(), agent);
            }
            ch.post_system_typed(&i18n::sys_connecting(&name), SystemKind::General);
            ch.state_changed();
        }

        match AcpClient::start(config, cwd, bus_tx, name.clone()).await {
            Ok((client, event_rx)) => {
                let session_id = client.session_id.clone();
                let client = Arc::new(tokio::sync::Mutex::new(client));
                {
                    let mut map = ctx.clients.lock().await;
                    map.insert(name.clone(), client.clone());
                }
                {
                    let mut ch = ctx.channel.lock().await;
                    if let Some(agent) = ch.agents.get_mut(&name) {
                        agent.status = AgentStatus::Idle;
                        agent.alive = true;
                        agent.session_id = session_id;
                    }
                    ch.post_system_typed(
                        &i18n::sys_online(&name, &adapter_name),
                        SystemKind::AgentOnline,
                    );
                    if is_main {
                        let mut entry =
                            acp_core::comm_log::entry(&ch.channel_id, "agent_lifecycle");
                        entry.from = Some(name.clone());
                        entry.transport = Some("internal".to_string());
                        entry.status = Some("online".to_string());
                        entry.detail = Some(format!("agent online via {adapter_name}"));
                        let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
                    }
                    ch.state_changed();
                }

                // Dispatch pending task if agent had one queued before connecting
                let pending = {
                    let mut ch = ctx.channel.lock().await;
                    ch.agents.get_mut(&name).and_then(|a| a.pending_task.take())
                };
                if let Some(task) = pending {
                    let ctx2 = ctx.clone();
                    let n = name.clone();
                    tokio::spawn(do_prompt(n, task, ctx2));
                }

                // Event listener for this agent
                spawn_agent_event_listener(
                    name.clone(),
                    is_main,
                    event_rx,
                    ctx.channel.clone(),
                    ctx.clients.clone(),
                    ctx.pending_waits.clone(),
                    ctx.wait_graph.clone(),
                );
            }
            Err(e) => {
                let mut ch = ctx.channel.lock().await;
                if let Some(agent) = ch.agents.get_mut(&name) {
                    agent.status = AgentStatus::Error;
                    if is_main {
                        agent.prompt_start_time = None;
                    }
                }
                ch.post_system_typed(&i18n::sys_connect_failed(&name, &e.to_string()), SystemKind::AgentError);
                ch.state_changed();
            }
        }
    });
}

/// Convenience wrapper matching the old `start_agent_bg` signature.
pub(crate) async fn start_agent_bg(
    name: String,
    adapter_name: String,
    ctx: BusContext,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
) {
    start_agent(name, adapter_name, false, ctx, bus_tx).await;
}

/// Spawn a tokio task that listens for `ClientEvent`s and updates the channel
/// state accordingly (streaming content, tool calls, agent exit).
fn spawn_agent_event_listener(
    name: String,
    is_main: bool,
    mut event_rx: mpsc::UnboundedReceiver<ClientEvent>,
    channel: Arc<tokio::sync::Mutex<Channel>>,
    clients: ClientMap,
    pending_waits: PendingWaits,
    wait_graph: SharedWaitGraph,
) {
    tokio::spawn(async move {
        while let Some(evt) = event_rx.recv().await {
            match evt {
                ClientEvent::SessionUpdate(params) => {
                    if let Some(update) = params.get("update") {
                        let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
                        let mut ch = channel.lock().await;
                        if let Some(agent) = ch.agents.get_mut(&name) {
                            match kind {
                                Some("agent_message_chunk") => {
                                    if let Some(content) = update.get("content") {
                                        if let Some(text) =
                                            content.get("text").and_then(|v| v.as_str())
                                        {
                                            agent.stream_buf.push_str(text);
                                            agent.activity = Some("typing".into());
                                        }
                                    }
                                }
                                Some("tool_call") => {
                                    let title = update
                                        .get("title")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("tool");
                                    let clean = Agent::clean_tool_name(title);
                                    agent.activity = Some(clean.clone());
                                    agent.push_tool_call(clean);
                                }
                                Some("tool_call_update") => {
                                    if let Some(title) =
                                        update.get("title").and_then(|v| v.as_str())
                                    {
                                        agent.activity = Some(Agent::clean_tool_name(title));
                                    }
                                }
                                Some("agent_thought_chunk") => {
                                    if let Some(content) = update.get("content") {
                                        if let Some(text) =
                                            content.get("text").and_then(|v| v.as_str())
                                        {
                                            agent.thinking_buf.push_str(text);
                                        }
                                    }
                                    agent.activity = Some("thinking".into());
                                }
                                Some("agent_message_start") => {
                                    agent.activity = None;
                                }
                                Some("agent_message_end") => {
                                    agent.activity = None;
                                    agent.finish_tool_calls();
                                    // Extract token usage if present
                                    if let Some(usage) = update.get("usage") {
                                        let input = usage
                                            .get("inputTokens")
                                            .and_then(|v| v.as_u64());
                                        let max = usage
                                            .get("maxContextTokens")
                                            .and_then(|v| v.as_u64())
                                            .or_else(|| {
                                                usage
                                                    .get("contextWindow")
                                                    .and_then(|v| v.as_u64())
                                            });
                                        if let (Some(i), Some(m)) = (input, max) {
                                            agent.context_tokens = Some((i, m));
                                        }
                                    }
                                }
                                _ => {}
                            }
                            ch.state_changed();
                        }
                    }
                }
                ClientEvent::Exited { code } => {
                    {
                        let mut map = clients.lock().await;
                        map.remove(&name);
                    }
                    // Clean up pending waits targeting this agent
                    {
                        let mut pw = pending_waits.lock().await;
                        if let Some(tx) = pw.remove(&name) {
                            let _ = tx.send(SendAndWaitResult {
                                ok: false,
                                reply_content: None,
                                from_agent: None,
                                error: Some(format!("agent {} exited", name)),
                            });
                        }
                    }
                    {
                        let mut wg = wait_graph.lock().await;
                        wg.cleanup_agent(&name);
                    }
                    let mut ch = channel.lock().await;
                    let abnormal = code != Some(0);
                    if let Some(agent) = ch.agents.get_mut(&name) {
                        agent.status = if abnormal {
                            AgentStatus::Error
                        } else {
                            AgentStatus::Disconnected
                        };
                        agent.alive = false;
                        if is_main {
                            agent.prompt_start_time = None;
                        }
                    }
                    let msg = if abnormal {
                        i18n::sys_abnormal_exit(&name, code)
                    } else {
                        i18n::sys_normal_exit(&name, code)
                    };
                    ch.post(i18n::SYS_SENDER_SYSTEM, &msg, true);
                    if is_main {
                        let mut entry =
                            acp_core::comm_log::entry(&ch.channel_id, "agent_lifecycle");
                        entry.from = Some(name.clone());
                        entry.transport = Some("internal".to_string());
                        entry.status = Some("offline".to_string());
                        entry.detail = Some(format!("agent exited with code={code:?}"));
                        let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
                    }
                    ch.state_changed();
                    break;
                }
            }
        }
    });
}
