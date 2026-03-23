use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use tokio::sync::{mpsc, Mutex};

use acp_core::adapter::{self, AdapterOpts};
use acp_core::agent::{Agent, AgentStatus};
use acp_core::channel::{Channel, ChannelEvent, MessageKind, MessageStatus, MessageTransport};
use acp_core::client::{
    AcpClient, BusEvent, BusSendResult, ClientEvent, CreateAgentResult, RemoveAgentResult,
    SendAndWaitResult,
};
use acp_core::router;

use crate::components::input::InputBox;
use crate::components::messages::MessagesView;
use crate::components::status_bar::{AgentDisplay, StatusBar, ToolCallDisplay};

use crate::layout::AppLayout;

type ClientHandle = Arc<tokio::sync::Mutex<AcpClient>>;
type ClientMap = Arc<Mutex<HashMap<String, ClientHandle>>>;

async fn append_comm_log(
    channel: &Arc<Mutex<Channel>>,
    mut entry: acp_core::comm_log::CommLogEntry,
) {
    let (cwd, channel_id) = {
        let ch = channel.lock().await;
        (ch.cwd.clone(), ch.channel_id.clone())
    };
    entry.channel_id = channel_id;
    let _ = acp_core::comm_log::append(&cwd, &entry).await;
}

type SharedScheduler = Arc<Mutex<acp_core::scheduler::Scheduler>>;

pub struct App {
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
    messages: MessagesView,
    status_bar: StatusBar,
    input: InputBox,
    should_quit: bool,
    cwd: String,
    default_adapter: String,
    cached_agents: Vec<AgentDisplay>,
    bus_tx: mpsc::UnboundedSender<BusEvent>,
    bus_rx: Option<mpsc::UnboundedReceiver<BusEvent>>,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
}

impl App {
    pub fn new(cwd: String) -> Self {
        let channel = Channel::new(cwd.clone());
        let (bus_tx, bus_rx) = mpsc::unbounded_channel();
        Self {
            channel: Arc::new(Mutex::new(channel)),
            clients: Arc::new(Mutex::new(HashMap::new())),
            messages: MessagesView::new(),
            status_bar: StatusBar::new(),
            input: InputBox::new(),
            should_quit: false,
            cwd,
            default_adapter: "claude".to_string(),
            cached_agents: Vec::new(),
            bus_tx,
            bus_rx: Some(bus_rx),
            socket_path: None,
            mcp_command: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            scheduler: Arc::new(Mutex::new(acp_core::scheduler::Scheduler::new())),
            pending_waits: Arc::new(Mutex::new(HashMap::new())),
            wait_graph: Arc::new(Mutex::new(acp_core::wait_graph::WaitGraph::new())),
        }
    }

    pub async fn run(&mut self, terminal: &mut ratatui::Terminal<impl Backend>) -> Result<()> {
        let mut event_rx = {
            let ch = self.channel.lock().await;
            ch.subscribe()
        };

        let mut bus_rx = self.bus_rx.take().expect("bus_rx already taken");

        // Start bus socket for agent-to-agent communication via MCP
        let channel_id = {
            let ch = self.channel.lock().await;
            ch.channel_id.clone()
        };
        match acp_core::bus_socket::start_bus_socket(&channel_id, self.bus_tx.clone()).await {
            Ok(path) => {
                self.socket_path = Some(path.to_string_lossy().to_string());
            }
            Err(e) => {
                tracing::warn!("failed to start bus socket: {e}");
            }
        }

        // Auto-start main agent
        self.start_agent("main".into(), self.default_adapter.clone())
            .await;

        let mut event_stream = crossterm::event::EventStream::new();
        use futures::StreamExt;

        loop {
            // Update input completions + collect streaming data (async, won't miss locks)
            self.update_completions().await;
            self.collect_frame_data().await;

            // Draw
            terminal.draw(|frame| self.draw(frame))?;

            // Handle events with proper async multiplexing
            tokio::select! {
                maybe_event = event_stream.next() => {
                    if let Some(Ok(evt)) = maybe_event {
                        match evt {
                            Event::Key(key) => self.handle_key(key).await,
                            Event::Paste(text) => self.input.insert_str(&text),
                            Event::Mouse(mouse) => {
                                use crossterm::event::{MouseEventKind};
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => self.messages.scroll_up(3),
                                    MouseEventKind::ScrollDown => self.messages.scroll_down(3),
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Ok(evt) = event_rx.recv() => {
                    match evt {
                        ChannelEvent::NewMessage { message, gap } => {
                            self.messages.push(&message, gap);
                            let h = terminal.size()?.height.saturating_sub(6);
                            self.messages.scroll_to_bottom(h);
                        }
                        ChannelEvent::StateChanged => {
                            let h = terminal.size()?.height.saturating_sub(6);
                            self.messages.scroll_to_bottom(h);
                        }
                        ChannelEvent::Closed => {
                            self.should_quit = true;
                        }
                    }
                }
                Some(bus_evt) = bus_rx.recv() => {
                    self.handle_bus_event(bus_evt).await;
                }
                // Redraw tick for streaming updates (no event needed)
                _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
            }

            if self.should_quit {
                break;
            }
        }

        // Cleanup: force-kill all agents immediately
        {
            let clients = self.clients.lock().await;
            for (_, client) in clients.iter() {
                if let Ok(c) = client.try_lock() {
                    c.force_kill();
                }
            }
        }

        // Remove bus socket
        if let Some(ref path) = self.socket_path {
            let _ = std::fs::remove_file(path);
        }

        Ok(())
    }

    async fn handle_bus_event(&self, event: BusEvent) {
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
                    let mut ch = self.channel.lock().await;
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
                append_comm_log(&self.channel, log_entry).await;
                if result.delivered {
                    let channel = self.channel.clone();
                    let clients = self.clients.clone();
                    let sp = self.socket_path.clone();
                    let mc = self.mcp_command.clone();
                    let sc = self.scheduler.clone();
                    let pw = self.pending_waits.clone();
                    let wg = self.wait_graph.clone();
                    let sender = from_agent.clone();
                    tokio::spawn(do_prompt_with_reply(to_agent, content, channel, clients, sp, mc, sc, sender, pw, wg));
                }
                let _ = reply_tx.send(result);
            }
            BusEvent::ListAgents { reply_tx, .. } => {
                let agents = {
                    let ch = self.channel.lock().await;
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
                let result = if from_agent != "main" {
                    CreateAgentResult {
                        ok: false,
                        error: Some("only main agent can create agents".into()),
                    }
                } else {
                    // Post task dispatch message immediately if task provided
                    if let Some(ref task_content) = task {
                        let mut ch = self.channel.lock().await;
                        ch.post_directed(
                            "main",
                            &name,
                            task_content,
                            MessageKind::Task,
                            MessageTransport::Internal,
                            MessageStatus::Delivered,
                        );
                    }
                    // Spawn agent in background
                    let channel = self.channel.clone();
                    let clients = self.clients.clone();
                    let bus_tx = Some(self.bus_tx.clone());
                    let sp = self.socket_path.clone();
                    let mc = self.mcp_command.clone();
                    let sc = self.scheduler.clone();
                    let pw = self.pending_waits.clone();
                    let wg = self.wait_graph.clone();
                    let task_clone = task.clone();
                    let name_clone = name.clone();
                    tokio::spawn(async move {
                        let pw2 = pw.clone();
                        let wg2 = wg.clone();
                        start_agent_bg(
                            name_clone.clone(),
                            adapter,
                            channel.clone(),
                            clients.clone(),
                            bus_tx,
                            sp.clone(),
                            mc.clone(),
                            sc.clone(),
                            pw,
                            wg,
                        )
                        .await;
                        // If task was provided, dispatch it after agent connects
                        if let Some(task_content) = task_clone {
                            do_prompt(name_clone, task_content, channel, clients, sp, mc, sc, pw2, wg2).await;
                        }
                    });
                    CreateAgentResult { ok: true, error: None }
                };
                let _ = reply_tx.send(result);
            }
            BusEvent::RemoveAgent {
                from_agent,
                name,
                reply_tx,
            } => {
                let result = if from_agent != "main" {
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
                        let mut map = self.clients.lock().await;
                        if let Some(client) = map.remove(&name) {
                            let mut c = client.lock().await;
                            c.stop().await;
                        }
                    }
                    let mut ch = self.channel.lock().await;
                    ch.remove_agent(&name);
                    RemoveAgentResult { ok: true, error: None }
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
                    let mut wg = self.wait_graph.lock().await;
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
                    let ch = self.channel.lock().await;
                    if !ch.agents.contains_key(&to_agent) {
                        // Remove the wait edge we just added
                        let mut wg = self.wait_graph.lock().await;
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
                    let mut ch = self.channel.lock().await;
                    let (conversation_id, reply_to) =
                        ch.resolve_reply_context(&from_agent, &to_agent);
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
                    let mut pw = self.pending_waits.lock().await;
                    pw.insert(from_agent.clone(), reply_tx);
                }

                // 5. Dispatch prompt to target agent
                let channel = self.channel.clone();
                let clients = self.clients.clone();
                let sp = self.socket_path.clone();
                let mc = self.mcp_command.clone();
                let sc = self.scheduler.clone();
                let pw2 = self.pending_waits.clone();
                let wg2 = self.wait_graph.clone();
                let to = to_agent.clone();
                let from = from_agent.clone();
                let prompt = format!("[来自 {from_agent} 的消息（等待回复）]\n{content}");
                tokio::spawn(do_prompt_with_reply(
                    to, prompt, channel, clients, sp, mc, sc, from.clone(), pw2, wg2,
                ));

                // 6. Timeout cleanup (independent task)
                let pw = self.pending_waits.clone();
                let wg = self.wait_graph.clone();
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
                    let mut ch = self.channel.lock().await;
                    ch.post_directed_with_refs(
                        &from_agent,
                        &to_agent,
                        &content,
                        MessageKind::Chat,
                        MessageTransport::BusTool,
                        MessageStatus::Delivered,
                        None,
                        in_reply_to,
                    )
                };

                // 2. Fulfill pending wait if the target agent was waiting
                let fulfilled = {
                    let mut pw = self.pending_waits.lock().await;
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
                    let mut wg = self.wait_graph.lock().await;
                    wg.remove_wait(&to_agent);
                }

                let _ = reply_tx.send(BusSendResult {
                    message_id: Some(message_id),
                    delivered: true,
                    error: None,
                });
            }
        }
    }

    async fn update_completions(&mut self) {
        if let Ok(ch) = self.channel.try_lock() {
            let agent_names: Vec<String> = ch.agents.keys().cloned().collect();
            let adapter_names: Vec<String> =
                adapter::list().iter().map(|s| s.to_string()).collect();
            self.input.set_completions(agent_names, adapter_names);
        }
    }

    /// Collect agent display info and streaming data before draw (async, reliable lock)
    async fn collect_frame_data(&mut self) {
        self.cached_agents.clear();
        self.cached_agents.push(AgentDisplay {
            name: "System".to_string(),
            status: "idle".to_string(),
            activity: None,
            adapter: None,
            session_id: None,
            prompt_start_time: None,
            waiting_reply_from: None,
            waiting_since: None,
            waiting_conversation_id: None,
            tool_calls: Vec::new(),
        });
        self.messages.streaming.clear();

        let ch = self.channel.lock().await;
        for (_, agent) in ch.agents.iter() {
            self.cached_agents.push(AgentDisplay {
                name: agent.name.clone(),
                status: agent.status.to_string(),
                activity: agent.activity.clone(),
                adapter: Some(agent.adapter_name.clone()),
                session_id: agent.session_id.clone(),
                prompt_start_time: agent.prompt_start_time,
                waiting_reply_from: agent.waiting_reply_from.clone(),
                waiting_since: agent.waiting_since,
                waiting_conversation_id: agent.waiting_conversation_id,
                tool_calls: agent.tool_calls.iter().map(|tc| ToolCallDisplay {
                    name: tc.name.clone(),
                    running: tc.status == acp_core::agent::ToolCallStatus::Running,
                }).collect(),
            });
            if agent.streaming && !agent.stream_buf.is_empty() {
                self.messages
                    .streaming
                    .push((agent.name.clone(), agent.stream_buf.clone()));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Compute text area width for input wrapping: total - sidebar - prompt - borders
        let area = frame.area();
        let sidebar_w: u16 = if area.width > 100 { 24 } else if area.width > 60 { 20 } else { 16 };
        let input_text_w = area.width.saturating_sub(sidebar_w + 3); // 2 for prompt + 1 for border
        let layout = AppLayout::new(area, self.input.visual_line_count(input_text_w));

        // Sidebar (agent list)
        self.status_bar
            .render(&self.cached_agents, layout.sidebar, frame.buffer_mut());

        // Messages (with streaming previews)
        self.messages.render(layout.messages, frame.buffer_mut());

        // Input
        self.input.render(layout.input, frame.buffer_mut());

        // Completion popup (rendered on top)
        self.input.render_popup(layout.input, frame.buffer_mut());

        // Cursor
        let (cx, cy) = self.input.cursor_position(layout.input);
        frame.set_cursor_position(Position::new(cx, cy));
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        match (key.modifiers, key.code) {
            (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
                self.should_quit = true;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('q')) => {
                // Cancel/interrupt the selected agent
                self.cancel_selected_agent().await;
            }
            (_, KeyCode::Tab) => {
                self.input.tab();
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.input.shift_tab();
            }
            (_, KeyCode::Esc) => {
                self.input.dismiss_popup();
            }
            (KeyModifiers::CONTROL, KeyCode::Enter) => {
                self.input.insert('\n');
            }
            (_, KeyCode::Enter) => {
                self.input.dismiss_popup();
                if !self.input.is_empty() {
                    let text = self.input.take();
                    self.handle_input(text).await;
                }
            }
            (_, KeyCode::Backspace) => self.input.backspace(),
            (_, KeyCode::Delete) => self.input.delete(),
            (_, KeyCode::Home) => self.input.move_home(),
            (_, KeyCode::End) => self.input.move_end(),
            (KeyModifiers::CONTROL, KeyCode::Char('j')) => self.messages.scroll_down(1),
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => self.messages.scroll_up(1),
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => self.messages.scroll_down(10),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => self.messages.scroll_up(10),
            (_, KeyCode::PageDown) => self.messages.scroll_down(20),
            (_, KeyCode::PageUp) => self.messages.scroll_up(20),
            // Tab switching: Ctrl+n / Ctrl+p / Shift+Arrow (must be before generic Left/Right)
            (KeyModifiers::CONTROL, KeyCode::Char('n')) | (KeyModifiers::SHIFT, KeyCode::Right) => {
                let count = self.agent_count().await + 1;
                self.status_bar.select_next(count);
                self.update_message_filter().await;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) | (KeyModifiers::SHIFT, KeyCode::Left) => {
                let count = self.agent_count().await + 1;
                self.status_bar.select_prev(count);
                self.update_message_filter().await;
            }
            (_, KeyCode::Left) => self.input.move_left(),
            (_, KeyCode::Right) => self.input.move_right(),
            (_, KeyCode::Char(c)) => self.input.insert(c),
            _ => {}
        }
    }

    async fn agent_count(&self) -> usize {
        let ch = self.channel.lock().await;
        ch.agents.len()
    }

    /// Returns the agent name for the currently selected tab, or None if "All" is selected.
    async fn selected_agent_name(&self) -> Option<String> {
        let idx = self.status_bar.selected;
        if idx == 0 {
            return None;
        }
        let ch = self.channel.lock().await;
        let names: Vec<String> = ch.agents.keys().cloned().collect();
        names.get(idx - 1).cloned()
    }

    async fn update_message_filter(&mut self) {
        let idx = self.status_bar.selected;
        if idx == 0 {
            // First tab = "All"
            self.messages.filter = None;
        } else {
            let ch = self.channel.lock().await;
            let names: Vec<String> = ch.agents.keys().cloned().collect();
            if let Some(name) = names.get(idx - 1) {
                self.messages.filter = Some(name.clone());
            }
        }
        self.messages.snap_to_bottom();
    }

    async fn cancel_selected_agent(&self) {
        let name = match self.selected_agent_name().await {
            Some(n) => n,
            None => {
                // System tab: cancel all running agents
                let clients = self.clients.lock().await;
                let mut cancelled = Vec::new();
                for (name, client) in clients.iter() {
                    if let Ok(c) = client.try_lock() {
                        if c.alive {
                            c.cancel().await;
                            cancelled.push(name.clone());
                        }
                    }
                }
                if !cancelled.is_empty() {
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", &format!("已中断: {}", cancelled.join(", ")), true);
                }
                return;
            }
        };

        let clients = self.clients.lock().await;
        if let Some(client) = clients.get(&name) {
            if let Ok(c) = client.try_lock() {
                c.cancel().await;
                let mut ch = self.channel.lock().await;
                ch.post("系统", &format!("已中断 {name}"), true);
            }
        }
    }

    async fn handle_input(&mut self, text: String) {
        if text.starts_with('/') {
            self.handle_command(&text).await;
            return;
        }

        // Determine target agent
        let has_mention = text.contains('@');
        if has_mention {
            // Post as broadcast, then route by @mentions
            let route_info = {
                let mut ch = self.channel.lock().await;
                let route_info = ch.post_message(
                    "you",
                    None,
                    &text,
                    MessageKind::Task,
                    MessageTransport::Ui,
                    MessageStatus::Sent,
                    None,
                    false,
                );
                let mut entry = acp_core::comm_log::entry("", "user_message");
                entry.from = Some("you".to_string());
                entry.transport = Some("ui".to_string());
                entry.status = Some("sent".to_string());
                entry.message_id = ch.messages.last().map(|m| m.id);
                entry.content = Some(text.clone());
                entry.detail = Some("user broadcast with mentions".to_string());
                let cwd = ch.cwd.clone();
                let channel_id = ch.channel_id.clone();
                drop(ch);
                entry.channel_id = channel_id;
                let _ = acp_core::comm_log::append(&cwd, &entry).await;
                route_info
            };
            if let Some((content, from)) = route_info {
                self.dispatch_to_agents(&content, &from).await;
            }
        } else {
            // Direct message to selected agent (like a normal chat)
            let target = self
                .selected_agent_name()
                .await
                .unwrap_or_else(|| "main".to_string());
            {
                let mut ch = self.channel.lock().await;
                let message_id = ch.post_directed(
                    "you",
                    &target,
                    &text,
                    MessageKind::Chat,
                    MessageTransport::Ui,
                    MessageStatus::Delivered,
                );
                let mut entry = acp_core::comm_log::entry(&ch.channel_id, "user_message");
                entry.from = Some("you".to_string());
                entry.to = Some(target.clone());
                entry.transport = Some("ui".to_string());
                entry.status = Some("delivered".to_string());
                entry.message_id = Some(message_id);
                entry.content = Some(text.clone());
                entry.detail = Some("user direct chat".to_string());
                let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
            }
            self.dispatch_single_agent(&target, &text).await;
        }
    }

    async fn dispatch_to_agents(&self, content: &str, from: &str) {
        let targets = {
            let ch = self.channel.lock().await;
            let names: Vec<String> = ch.agents.keys().cloned().collect();
            router::route(content, from, &names, 0)
        };

        for target in targets {
            let name = target.name.clone();
            // When message is from user, prepend context so agent knows to reply directly
            let content = if from == "you" || from == "你" {
                format!("[来自用户的消息，直接回复即可，不需要 @main]\n{}", target.content)
            } else {
                target.content.clone()
            };
            let channel = self.channel.clone();
            let clients = self.clients.clone();
            let sp = self.socket_path.clone();
            let mc = self.mcp_command.clone();
            let sc = self.scheduler.clone();
            let pw = self.pending_waits.clone();
            let wg = self.wait_graph.clone();

            tokio::spawn(async move {
                do_prompt(name.clone(), content.clone(), channel, clients, sp, mc, sc, pw, wg).await;
            });
        }
    }

    async fn dispatch_single_agent(&self, name: &str, content: &str) {
        let name = name.to_string();
        let content = content.to_string();
        let channel = self.channel.clone();
        let clients = self.clients.clone();
        let sp = self.socket_path.clone();
        let mc = self.mcp_command.clone();
        let sc = self.scheduler.clone();
        let pw = self.pending_waits.clone();
        let wg = self.wait_graph.clone();

        tokio::spawn(async move {
            do_prompt(name.clone(), content.clone(), channel, clients, sp, mc, sc, pw, wg).await;
        });
    }

    async fn handle_command(&mut self, text: &str) {
        let parts: Vec<&str> = text.splitn(4, ' ').collect();
        let cmd = parts[0];

        match cmd {
            "/add" => {
                if parts.len() < 3 {
                    let mut ch = self.channel.lock().await;
                    ch.post(
                        "系统",
                        "用法: /add <name> <adapter>\n可用 adapters: claude, c1, c2, gemini, codex",
                        true,
                    );
                    return;
                }
                let name = parts[1].to_string();
                let adapter_name = parts[2].to_string();
                let task = if parts.len() >= 4 {
                    Some(parts[3].to_string())
                } else {
                    None
                };
                self.start_agent(name.clone(), adapter_name).await;

                if let Some(task) = task {
                    // Post dispatch message immediately so it appears before
                    // any bus messages that arrive while the agent connects.
                    {
                        let mut ch = self.channel.lock().await;
                        ch.post_directed("main", &name, &task,
                            MessageKind::Task, MessageTransport::MentionRoute, MessageStatus::Delivered);
                    }
                    let ch = self.channel.clone();
                    let cl = self.clients.clone();
                    let sp = self.socket_path.clone();
                    let mc = self.mcp_command.clone();
                    let sc = self.scheduler.clone();
                    let pw = self.pending_waits.clone();
                    let wg = self.wait_graph.clone();
                    tokio::spawn(async move {
                        do_prompt(name, task, ch, cl, sp, mc, sc, pw, wg).await;
                    });
                }
            }
            "/remove" | "/rm" => {
                if parts.len() < 2 {
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", "用法: /remove <name>", true);
                    return;
                }
                let name = parts[1].to_string();
                if name == "main" {
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", "不能移除 main agent", true);
                    return;
                }
                {
                    let mut map = self.clients.lock().await;
                    if let Some(client) = map.remove(&name) {
                        let mut c = client.lock().await;
                        c.stop().await;
                    }
                }
                let mut ch = self.channel.lock().await;
                ch.remove_agent(&name);
            }
            "/list" | "/ls" => {
                let mut ch = self.channel.lock().await;
                let agents = ch.list_agents();
                let info: Vec<String> = agents
                    .iter()
                    .map(|a| format!("{} ({}) {}", a.name, a.kind, a.status))
                    .collect();
                if info.is_empty() {
                    ch.post("系统", "无 agent", true);
                } else {
                    ch.post("系统", &info.join("  |  "), true);
                }
            }
            "/adapters" => {
                let adapters = acp_core::adapter::list_detailed();
                let info: Vec<String> = adapters
                    .iter()
                    .map(|(name, desc)| format!("{name}: {desc}"))
                    .collect();
                let mut ch = self.channel.lock().await;
                ch.post("系统", &info.join("\n"), true);
            }
            "/cancel" => {
                if parts.len() < 2 {
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", "用法: /cancel <name>", true);
                    return;
                }
                let name = parts[1].to_string();
                let client = {
                    let map = self.clients.lock().await;
                    map.get(&name).cloned()
                };
                if let Some(client) = client {
                    let c = client.lock().await;
                    c.cancel().await;
                    drop(c);
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", &format!("已取消 {name}"), true);
                } else {
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", &format!("{name} 不存在"), true);
                }
            }
            "/save" => {
                let mut ch = self.channel.lock().await;
                match acp_core::store::save(&ch).await {
                    Ok(path) => {
                        ch.mark_saved();
                        ch.post("系统", &format!("已保存: {}", path.display()), true);
                    }
                    Err(e) => {
                        ch.post("系统", &format!("保存失败: {e}"), true);
                    }
                }
            }
            "/help" => {
                let mut ch = self.channel.lock().await;
                ch.post(
                    "系统",
                    "/add <name> <adapter>  添加 agent\n\
                     /remove <name>         移除 agent\n\
                     /list                  列出 agents\n\
                     /adapters              列出可用 adapters\n\
                     /cancel <name>         取消当前任务\n\
                     /save                  保存频道快照\n\
                     /quit                  退出\n\
                     消息中用 @name 路由到指定 agent\n\
                     无 @mention 的消息默认发给 main\n\
                     Tab 补全命令和 @agent",
                    true,
                );
            }
            "/quit" | "/q" => {
                self.should_quit = true;
            }
            _ => {
                let mut ch = self.channel.lock().await;
                ch.post("系统", &format!("未知命令: {cmd}，/help 查看帮助"), true);
            }
        }
    }

    async fn start_agent(&self, name: String, adapter_name: String) {
        let channel = self.channel.clone();
        let clients = self.clients.clone();
        let cwd = self.cwd.clone();
        let bus_tx = self.bus_tx.clone();
        let socket_path = self.socket_path.clone();
        let mcp_command = self.mcp_command.clone();
        let scheduler = self.scheduler.clone();
        let pending_waits = self.pending_waits.clone();
        let wait_graph = self.wait_graph.clone();

        tokio::spawn(async move {
            let opts = AdapterOpts {
                bus_mode: true,
                is_main: name == "main",
                agent_name: Some(name.clone()),
                channel_id: {
                    let ch = channel.lock().await;
                    Some(ch.channel_id.clone())
                },
                cwd: Some(cwd.clone()),
            };

            let mut config = match adapter::get(&adapter_name, &opts) {
                Ok(c) => c,
                Err(e) => {
                    let mut ch = channel.lock().await;
                    ch.post("系统", &format!("adapter 错误: {e}"), true);
                    return;
                }
            };
            config.socket_path = socket_path.clone();
            config.mcp_command = mcp_command.clone();

            let system_prompt = config.system_prompt.clone();

            {
                let mut ch = channel.lock().await;
                if name != "main" {
                    let agent =
                        Agent::new_spawned(name.clone(), adapter_name.clone(), system_prompt);
                    ch.agents.insert(name.clone(), agent);
                } else if let Some(agent) = ch.agents.get_mut("main") {
                    agent.adapter_name = adapter_name.clone();
                    agent.status = AgentStatus::Connecting;
                }
                ch.post("系统", &format!("{name} 正在连接…"), true);
                ch.state_changed();
            }

            match AcpClient::start(config, cwd, Some(bus_tx), name.clone()).await {
                Ok((client, mut event_rx)) => {
                    let session_id = client.session_id.clone();
                    let client = Arc::new(tokio::sync::Mutex::new(client));
                    {
                        let mut map = clients.lock().await;
                        map.insert(name.clone(), client.clone());
                    }
                    {
                        let mut ch = channel.lock().await;
                        if let Some(agent) = ch.agents.get_mut(&name) {
                            agent.status = AgentStatus::Idle;
                            agent.alive = true;
                            agent.session_id = session_id;
                        }
                        ch.post("系统", &format!("{name} ({adapter_name}) 已上线"), true);
                        let mut entry =
                            acp_core::comm_log::entry(&ch.channel_id, "agent_lifecycle");
                        entry.from = Some(name.clone());
                        entry.transport = Some("internal".to_string());
                        entry.status = Some("online".to_string());
                        entry.detail = Some(format!("agent online via {adapter_name}"));
                        let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
                        ch.state_changed();
                    }

                    // Dispatch pending task if agent had one queued before connecting
                    let pending = {
                        let mut ch = channel.lock().await;
                        ch.agents.get_mut(&name).and_then(|a| a.pending_task.take())
                    };
                    if let Some(task) = pending {
                        let ch = channel.clone();
                        let cl = clients.clone();
                        let sp2 = socket_path.clone();
                        let mc2 = mcp_command.clone();
                        let sc2 = scheduler.clone();
                        let pw2 = pending_waits.clone();
                        let wg2 = wait_graph.clone();
                        let n = name.clone();
                        tokio::spawn(do_prompt(n, task, ch, cl, sp2, mc2, sc2, pw2, wg2));
                    }

                    // Event listener for this agent
                    let channel2 = channel.clone();
                    let clients2 = clients.clone();
                    let name2 = name.clone();
                    let pending_waits2 = pending_waits.clone();
                    let wait_graph2 = wait_graph.clone();
                    tokio::spawn(async move {
                        while let Some(evt) = event_rx.recv().await {
                            match evt {
                                ClientEvent::SessionUpdate(params) => {
                                    if let Some(update) = params.get("update") {
                                        let kind =
                                            update.get("sessionUpdate").and_then(|v| v.as_str());
                                        let mut ch = channel2.lock().await;
                                        if let Some(agent) = ch.agents.get_mut(&name2) {
                                            match kind {
                                                Some("agent_message_chunk") => {
                                                    if let Some(content) = update.get("content") {
                                                        if let Some(text) = content
                                                            .get("text")
                                                            .and_then(|v| v.as_str())
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
                                                    agent.activity = Some(title.to_string());
                                                    agent.push_tool_call(title.to_string());
                                                }
                                                Some("tool_call_update") => {
                                                    if let Some(title) =
                                                        update.get("title").and_then(|v| v.as_str())
                                                    {
                                                        agent.activity = Some(title.to_string());
                                                    }
                                                }
                                                Some("agent_thought_chunk") => {
                                                    agent.activity = Some("thinking".into());
                                                }
                                                Some("agent_message_start") => {
                                                    agent.activity = None;
                                                }
                                                Some("agent_message_end") => {
                                                    agent.activity = None;
                                                    agent.finish_tool_calls();
                                                }
                                                _ => {
                                                    // Don't change activity for unknown update types
                                                }
                                            }
                                            ch.state_changed();
                                        }
                                    }
                                }
                                ClientEvent::Exited { code } => {
                                    {
                                        let mut map = clients2.lock().await;
                                        map.remove(&name2);
                                    }
                                    // Clean up pending waits targeting this agent
                                    {
                                        let mut pw = pending_waits2.lock().await;
                                        if let Some(tx) = pw.remove(&name2) {
                                            let _ = tx.send(SendAndWaitResult {
                                                ok: false,
                                                reply_content: None,
                                                from_agent: None,
                                                error: Some(format!("agent {} exited", name2)),
                                            });
                                        }
                                    }
                                    {
                                        let mut wg = wait_graph2.lock().await;
                                        wg.cleanup_agent(&name2);
                                    }
                                    let mut ch = channel2.lock().await;
                                    let abnormal = code != Some(0);
                                    if let Some(agent) = ch.agents.get_mut(&name2) {
                                        agent.status = if abnormal {
                                            AgentStatus::Error
                                        } else {
                                            AgentStatus::Disconnected
                                        };
                                        agent.alive = false;
                                        agent.prompt_start_time = None;
                                    }
                                    let msg = if abnormal {
                                        format!("{name2} 异常退出 (code={code:?})")
                                    } else {
                                        format!("{name2} 退出 (code={code:?})")
                                    };
                                    ch.post("系统", &msg, true);
                                    let mut entry = acp_core::comm_log::entry(
                                        &ch.channel_id,
                                        "agent_lifecycle",
                                    );
                                    entry.from = Some(name2.clone());
                                    entry.transport = Some("internal".to_string());
                                    entry.status = Some("offline".to_string());
                                    entry.detail = Some(format!("agent exited with code={code:?}"));
                                    let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
                                    ch.state_changed();
                                    break;
                                }
                            }
                        }
                    });
                }
                Err(e) => {
                    let mut ch = channel.lock().await;
                    if let Some(agent) = ch.agents.get_mut(&name) {
                        agent.status = AgentStatus::Error;
                        agent.prompt_start_time = None;
                    }
                    ch.post("系统", &format!("{name} 连接失败: {e}"), true);
                    ch.state_changed();
                }
            }
        });
    }
}

/// Execute a prompt to an agent and post the reply back to the channel
fn do_prompt(
    name: String,
    content: String,
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, channel, clients, socket_path, mcp_command, scheduler, None, pending_waits, wait_graph))
}

fn do_prompt_with_reply(
    name: String,
    content: String,
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    reply_to: String,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, channel, clients, socket_path, mcp_command, scheduler, Some(reply_to), pending_waits, wait_graph))
}

async fn do_prompt_inner(
    name: String,
    content: String,
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    reply_to: Option<String>,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
) {
    // Scheduler gate: serialize prompts to main agent
    if name == "main" {
        let should_send = {
            let mut sched = scheduler.lock().await;
            match sched.push_to_main_with_reply(content.clone(), None, reply_to.clone()) {
                Ok(immediate) => immediate,
                Err(msg) => {
                    let mut ch = channel.lock().await;
                    ch.post("系统", &msg, true);
                    return;
                }
            }
        };
        if !should_send {
            // Queued — will be dispatched when current main prompt finishes
            return;
        }
    }
    // Build payload (system prompt is injected via ACP _meta at session creation)
    let payload = {
        let mut ch = channel.lock().await;
        let p = if let Some(agent) = ch.agents.get_mut(&name) {
            agent.status = AgentStatus::Streaming;
            agent.streaming = true;
            agent.stream_buf.clear();
            agent.activity = Some("receiving".into());
            agent.current_task = Some(content.chars().take(100).collect());
            // First prompt: inject identity reminder so agent knows who it is
            let text = if !agent.prompted {
                format!(
                    "[身份: 你是 {name}，acp-bus 频道中的 agent。你的 adapter 是 {}。]\n\n{content}",
                    agent.adapter_name
                )
            } else {
                content.clone()
            };
            agent.prompted = true;
            agent.prompt_start_time = Some(chrono::Utc::now().timestamp());
            Some(text)
        } else {
            None
        };
        ch.state_changed();
        match p {
            Some(p) => p,
            None => return,
        }
    };

    // Get client handle — wait if agent is still connecting
    let client = {
        let mut client = None;
        for _ in 0..60 {
            let map = clients.lock().await;
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
            let mut ch = channel.lock().await;
            ch.post("系统", &format!("{name} 未连接（等待超时），任务已暂存"), true);
            if let Some(agent) = ch.agents.get_mut(&name) {
                agent.status = AgentStatus::Idle;
                agent.streaming = false;
                agent.prompt_start_time = None;
                agent.pending_task = Some(content);
            }
            ch.state_changed();
            return;
        }
    };

    // Dispatch messages are posted by the caller (handle_input or do_prompt_inner routing).
    // No need to post again here.

    // Execute prompt
    let stop_reason = {
        let c = client.lock().await;
        c.prompt(&payload).await
    };

    // Collect reply from stream_buf
    let reply = {
        let mut ch = channel.lock().await;
        let buf = if let Some(agent) = ch.agents.get_mut(&name) {
            agent.streaming = false;
            agent.status = AgentStatus::Idle;
            agent.activity = None;
            agent.prompt_start_time = None;
            agent.current_task = None;
            std::mem::take(&mut agent.stream_buf)
        } else {
            return;
        };
        ch.state_changed();
        buf
    };

    match stop_reason {
        Ok(_) => {
            if !reply.is_empty() {
                // Parse and execute /add commands from agent output
                let added_agents = execute_agent_commands(&reply, &channel, &clients, None, socket_path.clone(), mcp_command.clone(), scheduler.clone(), pending_waits.clone(), wait_graph.clone()).await;

                // Check if reply has @mentions that need routing
                let known_agents = {
                    let ch = channel.lock().await;
                    ch.agents.keys().cloned().collect::<Vec<_>>()
                };
                let targets = router::route(&reply, &name, &known_agents, 1);

                if targets.is_empty() {
                    if let Some(ref sender) = reply_to {
                        // Auto-route reply back to the agent who sent the bus message
                        {
                            let mut ch = channel.lock().await;
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
                        // Prompt the sender with the reply
                        let ch = channel.clone();
                        let cl = clients.clone();
                        let sp = socket_path.clone();
                        let mc = mcp_command.clone();
                        let sc = scheduler.clone();
                        let pw = pending_waits.clone();
                        let wg = wait_graph.clone();
                        let sender = sender.clone();
                        let reply_content = format!("[来自 {name} 的回复]\n{reply}");
                        tokio::spawn(do_prompt(sender, reply_content, ch, cl, sp, mc, sc, pw, wg));
                    } else {
                        // No routing, no reply_to — post full reply as broadcast
                        let mut ch = channel.lock().await;
                        ch.post(&name, &reply, true);
                    }
                } else {
                    // Has @mentions — skip broadcast to avoid duplicate messages.
                    // Only post directed messages to each target below.

                    // Wait for newly added agents
                    if !added_agents.is_empty() {
                        wait_for_agents(&added_agents, &clients, 30).await;
                    }

                    // Post per-agent segments and dispatch
                    for target in targets {
                        let tname = target.name.clone();
                        let tcontent = target.content.clone();

                        // Post dispatch message visible in agent's tab
                        {
                            let mut ch = channel.lock().await;
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

                        let ch = channel.clone();
                        let cl = clients.clone();
                        let sp = socket_path.clone();
                        let mc = mcp_command.clone();
                        let sc = scheduler.clone();
                        let pw = pending_waits.clone();
                        let wg = wait_graph.clone();
                        tokio::spawn(do_prompt(tname, tcontent, ch, cl, sp, mc, sc, pw, wg));
                    }
                }
            } else {
                let mut ch = channel.lock().await;
                ch.post(&name, "(完成，无文本输出)", true);
            }
            {
                let mut ch = channel.lock().await;
                ch.post("系统", &format!("{name} 已完成"), true);
            }
        }
        Err(e) => {
            let mut ch = channel.lock().await;
            ch.post("系统", &format!("{name} 出错: {e}"), true);
            if let Some(agent) = ch.agents.get_mut(&name) {
                agent.status = AgentStatus::Error;
            }
            ch.state_changed();
        }
    }

    // Scheduler: if this was main, drain the next queued message
    if name == "main" {
        let next = {
            let mut sched = scheduler.lock().await;
            sched.main_done()
        };
        if let Some(queued) = next {
            let ch = channel.clone();
            let cl = clients.clone();
            let sp = socket_path.clone();
            let mc = mcp_command.clone();
            let sc = scheduler.clone();
            let pw = pending_waits.clone();
            let wg = wait_graph.clone();
            if let Some(reply_to) = queued.reply_to {
                tokio::spawn(do_prompt_with_reply("main".to_string(), queued.content, ch, cl, sp, mc, sc, reply_to, pw, wg));
            } else {
                tokio::spawn(do_prompt("main".to_string(), queued.content, ch, cl, sp, mc, sc, pw, wg));
            }
        }
    }
}

/// Scan agent output for `/add name adapter` commands and execute them.
/// Returns names of newly added agents.
async fn execute_agent_commands(
    reply: &str,
    channel: &Arc<Mutex<Channel>>,
    clients: &ClientMap,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
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
                let continuation: String = trimmed
                    .lines()
                    .skip(1)
                    .collect::<Vec<_>>()
                    .join("\n");
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
                    let ch = channel.lock().await;
                    ch.agents.contains_key(&agent_name)
                };
                if !exists {
                    start_agent_bg(
                        agent_name.clone(),
                        adapter_name,
                        channel.clone(),
                        clients.clone(),
                        bus_tx.clone(),
                        socket_path.clone(),
                        mcp_command.clone(),
                        scheduler.clone(),
                        pending_waits.clone(),
                        wait_graph.clone(),
                    )
                    .await;
                    added.push(agent_name.clone());

                    if let Some(task) = task {
                        let ch = channel.clone();
                        let cl = clients.clone();
                        let sp = socket_path.clone();
                        let mc = mcp_command.clone();
                        let sc = scheduler.clone();
                        let pw = pending_waits.clone();
                        let wg = wait_graph.clone();
                        tokio::spawn(async move {
                            wait_for_agents(&[agent_name.clone()], &cl, 30).await;
                            {
                                let mut chan = ch.lock().await;
                                chan.post_directed("main", &agent_name, &task,
                                    MessageKind::Task, MessageTransport::MentionRoute, MessageStatus::Delivered);
                            }
                            do_prompt(agent_name, task, ch, cl, sp, mc, sc, pw, wg).await;
                        });
                    }
                }
            }
        } else if trimmed.starts_with("/remove ") {
            let parts: Vec<&str> = trimmed.splitn(2, ' ').collect();
            if parts.len() >= 2 {
                let agent_name = parts[1].trim();
                if agent_name != "main" {
                    let mut map = clients.lock().await;
                    if let Some(client) = map.remove(agent_name) {
                        let mut c = client.lock().await;
                        c.stop().await;
                    }
                    drop(map);
                    let mut ch = channel.lock().await;
                    ch.remove_agent(agent_name);
                }
            }
        }
    }
    added
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

/// Spawn an agent in the background (used by both App and agent auto-commands)
async fn start_agent_bg(
    name: String,
    adapter_name: String,
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
    socket_path: Option<String>,
    mcp_command: Option<String>,
    scheduler: SharedScheduler,
    pending_waits: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>,
    wait_graph: Arc<Mutex<acp_core::wait_graph::WaitGraph>>,
) {
    let cwd = {
        let ch = channel.lock().await;
        ch.cwd.clone()
    };

    tokio::spawn(async move {
        let opts = AdapterOpts {
            bus_mode: true,
            is_main: false,
            agent_name: Some(name.clone()),
            channel_id: {
                let ch = channel.lock().await;
                Some(ch.channel_id.clone())
            },
            cwd: Some(cwd.clone()),
        };

        let mut config = match adapter::get(&adapter_name, &opts) {
            Ok(c) => c,
            Err(e) => {
                let mut ch = channel.lock().await;
                ch.post("系统", &format!("adapter 错误: {e}"), true);
                return;
            }
        };
        config.socket_path = socket_path.clone();
        config.mcp_command = mcp_command.clone();

        let system_prompt = config.system_prompt.clone();

        {
            let mut ch = channel.lock().await;
            let agent = Agent::new_spawned(name.clone(), adapter_name.clone(), system_prompt);
            ch.agents.insert(name.clone(), agent);
            ch.post("系统", &format!("{name} 正在连接…"), true);
            ch.state_changed();
        }

        match AcpClient::start(config, cwd, bus_tx, name.clone()).await {
            Ok((client, mut event_rx)) => {
                let session_id = client.session_id.clone();
                let client = Arc::new(tokio::sync::Mutex::new(client));
                {
                    let mut map = clients.lock().await;
                    map.insert(name.clone(), client.clone());
                }
                {
                    let mut ch = channel.lock().await;
                    if let Some(agent) = ch.agents.get_mut(&name) {
                        agent.status = AgentStatus::Idle;
                        agent.alive = true;
                        agent.session_id = session_id;
                    }
                    ch.post("系统", &format!("{name} ({adapter_name}) 已上线"), true);
                    ch.state_changed();
                }

                // Dispatch pending task if agent had one queued before connecting
                let pending_bg = {
                    let mut ch = channel.lock().await;
                    ch.agents.get_mut(&name).and_then(|a| a.pending_task.take())
                };
                if let Some(task) = pending_bg {
                    let ch = channel.clone();
                    let cl = clients.clone();
                    let sp2 = socket_path.clone();
                    let mc2 = mcp_command.clone();
                    let sc2 = scheduler.clone();
                    let pw2 = pending_waits.clone();
                    let wg2 = wait_graph.clone();
                    let n = name.clone();
                    tokio::spawn(do_prompt(n, task, ch, cl, sp2, mc2, sc2, pw2, wg2));
                }

                let channel2 = channel.clone();
                let clients2 = clients.clone();
                let name2 = name.clone();
                let pending_waits2 = pending_waits.clone();
                let wait_graph2 = wait_graph.clone();
                tokio::spawn(async move {
                    while let Some(evt) = event_rx.recv().await {
                        match evt {
                            ClientEvent::SessionUpdate(params) => {
                                if let Some(update) = params.get("update") {
                                    let kind = update.get("sessionUpdate").and_then(|v| v.as_str());
                                    let mut ch = channel2.lock().await;
                                    if let Some(agent) = ch.agents.get_mut(&name2) {
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
                                                agent.activity = Some(title.to_string());
                                            }
                                            Some("tool_call_update") => {
                                                if let Some(title) =
                                                    update.get("title").and_then(|v| v.as_str())
                                                {
                                                    agent.activity = Some(title.to_string());
                                                }
                                            }
                                            Some("agent_thought_chunk") => {
                                                agent.activity = Some("thinking".into());
                                            }
                                            Some("agent_message_start" | "agent_message_end") => {
                                                agent.activity = None;
                                            }
                                            _ => {}
                                        }
                                        ch.state_changed();
                                    }
                                }
                            }
                            ClientEvent::Exited { code } => {
                                {
                                    let mut map = clients2.lock().await;
                                    map.remove(&name2);
                                }
                                // Clean up pending waits targeting this agent
                                {
                                    let mut pw = pending_waits2.lock().await;
                                    if let Some(tx) = pw.remove(&name2) {
                                        let _ = tx.send(SendAndWaitResult {
                                            ok: false,
                                            reply_content: None,
                                            from_agent: None,
                                            error: Some(format!("agent {} exited", name2)),
                                        });
                                    }
                                }
                                {
                                    let mut wg = wait_graph2.lock().await;
                                    wg.cleanup_agent(&name2);
                                }
                                let mut ch = channel2.lock().await;
                                let abnormal = code != Some(0);
                                if let Some(agent) = ch.agents.get_mut(&name2) {
                                    agent.status = if abnormal {
                                        AgentStatus::Error
                                    } else {
                                        AgentStatus::Disconnected
                                    };
                                    agent.alive = false;
                                }
                                let msg = if abnormal {
                                    format!("{name2} 异常退出 (code={code:?})")
                                } else {
                                    format!("{name2} 退出 (code={code:?})")
                                };
                                ch.post("系统", &msg, true);
                                ch.state_changed();
                                break;
                            }
                        }
                    }
                });
            }
            Err(e) => {
                let mut ch = channel.lock().await;
                if let Some(agent) = ch.agents.get_mut(&name) {
                    agent.status = AgentStatus::Error;
                }
                ch.post("系统", &format!("{name} 连接失败: {e}"), true);
                ch.state_changed();
            }
        }
    });
}
