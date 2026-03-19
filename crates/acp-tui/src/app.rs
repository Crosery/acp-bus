use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use tokio::sync::{mpsc, Mutex};

use acp_core::adapter::{self, AdapterOpts};
use acp_core::agent::{Agent, AgentStatus};
use acp_core::channel::{Channel, ChannelEvent, MessageKind, MessageStatus, MessageTransport};
use acp_core::client::{AcpClient, BusEvent, BusSendResult, ClientEvent};
use acp_core::router;

use crate::components::input::InputBox;
use crate::components::messages::MessagesView;
use crate::components::status_bar::{AgentDisplay, StatusBar};

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

        // Cleanup
        let mut clients = self.clients.lock().await;
        for (_, client) in clients.drain() {
            let mut c = client.lock().await;
            c.stop().await;
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
                    if !ch.agents.contains_key(&to_agent) {
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
                    tokio::spawn(do_prompt(to_agent, content, channel, clients));
                }
                let _ = reply_tx.send(result);
            }
            BusEvent::ListAgents { reply_tx, .. } => {
                let agents = {
                    let ch = self.channel.lock().await;
                    ch.agents
                        .iter()
                        .map(|(name, agent)| acp_core::client::AgentInfo {
                            name: name.clone(),
                            status: agent.status.to_string(),
                            adapter: agent.adapter_name.clone(),
                        })
                        .collect()
                };
                let _ = reply_tx.send(agents);
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
            });
            if agent.streaming && !agent.stream_buf.is_empty() {
                self.messages
                    .streaming
                    .push((agent.name.clone(), agent.stream_buf.clone()));
            }
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let layout = AppLayout::new(frame.area());

        // Messages (with streaming previews)
        self.messages.render(layout.messages, frame.buffer_mut());

        // Status bar
        self.status_bar
            .render(&self.cached_agents, layout.status_bar, frame.buffer_mut());

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
            (_, KeyCode::Tab) => {
                self.input.tab();
            }
            (KeyModifiers::SHIFT, KeyCode::BackTab) => {
                self.input.shift_tab();
            }
            (_, KeyCode::Esc) => {
                self.input.dismiss_popup();
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
        self.messages.scroll_to_top();
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
            let content = target.content.clone();
            let channel = self.channel.clone();
            let clients = self.clients.clone();

            tokio::spawn(async move {
                do_prompt(name.clone(), content.clone(), channel, clients).await;
            });
        }
    }

    async fn dispatch_single_agent(&self, name: &str, content: &str) {
        let name = name.to_string();
        let content = content.to_string();
        let channel = self.channel.clone();
        let clients = self.clients.clone();

        tokio::spawn(async move {
            do_prompt(name.clone(), content.clone(), channel, clients).await;
        });
    }

    async fn handle_command(&mut self, text: &str) {
        let parts: Vec<&str> = text.splitn(3, ' ').collect();
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
                self.start_agent(name, adapter_name).await;
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
                let ch = self.channel.lock().await;
                let agents = ch.list_agents();
                let info: Vec<String> = agents
                    .iter()
                    .map(|a| format!("{} ({}) {}", a.name, a.kind, a.status))
                    .collect();
                drop(ch);
                let mut ch = self.channel.lock().await;
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
                let map = self.clients.lock().await;
                if let Some(client) = map.get(&name) {
                    let c = client.lock().await;
                    c.cancel().await;
                    drop(c);
                    drop(map);
                    let mut ch = self.channel.lock().await;
                    ch.post("系统", &format!("已取消 {name}"), true);
                } else {
                    drop(map);
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
            config.socket_path = socket_path;

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

                    // Event listener for this agent
                    let channel2 = channel.clone();
                    let clients2 = clients.clone();
                    let name2 = name.clone();
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
                                                Some(
                                                    "agent_message_start" | "agent_message_end",
                                                ) => {
                                                    agent.activity = None;
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
                                    let mut ch = channel2.lock().await;
                                    if let Some(agent) = ch.agents.get_mut(&name2) {
                                        agent.status = AgentStatus::Disconnected;
                                        agent.alive = false;
                                        agent.prompt_start_time = None;
                                    }
                                    ch.post("系统", &format!("{name2} 退出 (code={code:?})"), true);
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
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    Box::pin(do_prompt_inner(name, content, channel, clients))
}

async fn do_prompt_inner(
    name: String,
    content: String,
    channel: Arc<Mutex<Channel>>,
    clients: ClientMap,
) {
    // Build payload (system prompt is injected via ACP _meta at session creation)
    let payload = {
        let mut ch = channel.lock().await;
        let p = if let Some(agent) = ch.agents.get_mut(&name) {
            agent.status = AgentStatus::Streaming;
            agent.streaming = true;
            agent.stream_buf.clear();
            agent.activity = Some("receiving".into());
            agent.prompted = true;
            agent.prompt_start_time = Some(chrono::Utc::now().timestamp());
            Some(content.clone())
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
            ch.post("系统", &format!("{name} 未连接（等待超时）"), true);
            if let Some(agent) = ch.agents.get_mut(&name) {
                agent.status = AgentStatus::Idle;
                agent.streaming = false;
                agent.prompt_start_time = None;
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
                let added_agents = execute_agent_commands(&reply, &channel, &clients, None).await;

                // Check if reply has @mentions that need routing
                let known_agents = {
                    let ch = channel.lock().await;
                    ch.agents.keys().cloned().collect::<Vec<_>>()
                };
                let targets = router::route(&reply, &name, &known_agents, 1);

                if targets.is_empty() {
                    // No routing — post full reply as broadcast
                    let mut ch = channel.lock().await;
                    ch.post(&name, &reply, true);
                } else {
                    // Has @mentions — post clean summary (strip /add and @mention lines)
                    let clean: String = reply
                        .lines()
                        .filter(|l| {
                            let t = l.trim();
                            !t.starts_with('/') && !t.starts_with('@')
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    let clean = clean.trim();
                    if !clean.is_empty() {
                        let mut ch = channel.lock().await;
                        ch.post(&name, clean, true);
                    }

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
                        tokio::spawn(do_prompt(tname, tcontent, ch, cl));
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
}

/// Scan agent output for `/add name adapter` commands and execute them.
/// Returns names of newly added agents.
async fn execute_agent_commands(
    reply: &str,
    channel: &Arc<Mutex<Channel>>,
    clients: &ClientMap,
    bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
) -> Vec<String> {
    let mut added = Vec::new();
    for line in reply.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("/add ") {
            let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
            if parts.len() >= 3 {
                let agent_name = parts[1].to_string();
                let adapter_name = parts[2].to_string();
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
                    )
                    .await;
                    added.push(agent_name);
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

        let config = match adapter::get(&adapter_name, &opts) {
            Ok(c) => c,
            Err(e) => {
                let mut ch = channel.lock().await;
                ch.post("系统", &format!("adapter 错误: {e}"), true);
                return;
            }
        };

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

                let channel2 = channel.clone();
                let clients2 = clients.clone();
                let name2 = name.clone();
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
                                let mut ch = channel2.lock().await;
                                if let Some(agent) = ch.agents.get_mut(&name2) {
                                    agent.status = AgentStatus::Disconnected;
                                    agent.alive = false;
                                }
                                ch.post("系统", &format!("{name2} 退出 (code={code:?})"), true);
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
