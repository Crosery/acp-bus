mod bus_events;
pub(crate) mod clipboard;
mod commands;
mod image;
mod lifecycle;
mod prompting;

pub(crate) use clipboard::{PendingImage, PendingImages};
pub(crate) use lifecycle::start_agent_bg;
pub(crate) use prompting::{dispatch_group_sequential, do_prompt, do_prompt_with_reply, is_main_instance};

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use tokio::sync::{mpsc, Mutex};

use acp_core::adapter;
use acp_core::channel::{Channel, ChannelEvent, MessageKind, MessageStatus, MessageTransport};
use acp_core::client::{AcpClient, BusEvent, SendAndWaitResult};
use acp_core::router;

use crate::components::input::InputBox;
use crate::components::messages::MessagesView;
use crate::components::status_bar::{AgentDisplay, StatusBar, ToolCallDisplay};

use crate::layout::AppLayout;

pub(crate) type ClientHandle = Arc<tokio::sync::Mutex<AcpClient>>;
pub(crate) type ClientMap = Arc<Mutex<HashMap<String, ClientHandle>>>;
pub(crate) type PendingWaits =
    Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<SendAndWaitResult>>>>;
pub(crate) type SharedWaitGraph = Arc<Mutex<acp_core::wait_graph::WaitGraph>>;
pub(crate) type SharedScheduler = Arc<Mutex<acp_core::fair_scheduler::FairScheduler>>;
pub(crate) type SharedPendingTasks = Arc<Mutex<acp_core::pending_tasks::PendingTasks>>;

/// Shared runtime context for agent coordination.
/// Bundled to avoid passing 7+ individual params to free functions.
#[derive(Clone)]
pub struct BusContext {
    pub channel: Arc<Mutex<Channel>>,
    pub clients: ClientMap,
    pub scheduler: SharedScheduler,
    pub socket_path: Option<String>,
    pub mcp_command: Option<String>,
    pub pending_waits: PendingWaits,
    pub wait_graph: SharedWaitGraph,
    pub pending_tasks: SharedPendingTasks,
}

pub struct App {
    ctx: BusContext,
    messages: MessagesView,
    status_bar: StatusBar,
    input: InputBox,
    should_quit: bool,
    sidebar_collapsed: bool,
    cwd: String,
    default_adapter: String,
    cached_agents: Vec<AgentDisplay>,
    bus_tx: mpsc::UnboundedSender<BusEvent>,
    bus_rx: Option<mpsc::UnboundedReceiver<BusEvent>>,
    cached_groups: Vec<crate::components::status_bar::GroupDisplay>,
    last_auto_save: i64,
    pending_images: PendingImages,
}

impl App {
    pub fn new(cwd: String) -> Self {
        let channel = Channel::new(cwd.clone());
        let (bus_tx, bus_rx) = mpsc::unbounded_channel();
        let ctx = BusContext {
            channel: Arc::new(Mutex::new(channel)),
            clients: Arc::new(Mutex::new(HashMap::new())),
            scheduler: Arc::new(Mutex::new(acp_core::fair_scheduler::FairScheduler::new())),
            socket_path: None,
            mcp_command: std::env::current_exe()
                .ok()
                .map(|p| p.to_string_lossy().to_string()),
            pending_waits: Arc::new(Mutex::new(HashMap::new())),
            wait_graph: Arc::new(Mutex::new(acp_core::wait_graph::WaitGraph::new())),
            pending_tasks: Arc::new(Mutex::new(acp_core::pending_tasks::PendingTasks::new())),
        };
        Self {
            ctx,
            messages: MessagesView::new(),
            status_bar: StatusBar::new(),
            input: InputBox::new(),
            should_quit: false,
            sidebar_collapsed: false,
            cwd,
            default_adapter: "claude".to_string(),
            cached_agents: Vec::new(),
            cached_groups: Vec::new(),
            last_auto_save: 0,
            pending_images: PendingImages::default(),
            bus_tx,
            bus_rx: Some(bus_rx),
        }
    }

    pub async fn run(&mut self, terminal: &mut ratatui::Terminal<impl Backend>) -> Result<()> {
        let mut event_rx = {
            let ch = self.ctx.channel.lock().await;
            ch.subscribe()
        };

        let mut bus_rx = self.bus_rx.take().expect("bus_rx already taken");

        // Start bus socket for agent-to-agent communication via MCP
        let channel_id = {
            let ch = self.ctx.channel.lock().await;
            ch.channel_id.clone()
        };
        match acp_core::bus_socket::start_bus_socket(&channel_id, self.bus_tx.clone()).await {
            Ok(path) => {
                self.ctx.socket_path = Some(path.to_string_lossy().to_string());
            }
            Err(e) => {
                tracing::warn!("failed to start bus socket: {e}");
            }
        }

        // Start main agent — additional instances spawn elastically on demand
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
                            Event::Paste(text) => {
                                if text.is_empty() {
                                    // Empty paste — likely an image in clipboard
                                    self.try_paste_image().await;
                                } else {
                                    self.input.insert_str(&text);
                                }
                            }
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
                // Adaptive redraw: fast during streaming, slow when idle
                _ = tokio::time::sleep(std::time::Duration::from_millis(
                    if self.messages.streaming.is_empty() && self.messages.thinking.is_empty() {
                        200
                    } else {
                        50
                    }
                )) => {}
            }

            // Drain any pending channel events before next redraw
            // (ensures user messages appear immediately after Enter)
            while let Ok(evt) = event_rx.try_recv() {
                match evt {
                    ChannelEvent::NewMessage { message, gap } => {
                        self.messages.push(&message, gap);
                    }
                    ChannelEvent::StateChanged => {}
                    ChannelEvent::Closed => {
                        self.should_quit = true;
                    }
                }
            }

            // Periodic auto-save (every 60s) so logs are available even if TUI crashes
            {
                let now = chrono::Utc::now().timestamp();
                if now - self.last_auto_save >= 60 {
                    self.last_auto_save = now;
                    let ch = self.ctx.channel.lock().await;
                    if !ch.messages.is_empty() {
                        let _ = acp_core::store::export_log(&ch).await;
                        let _ = acp_core::store::save(&ch).await;
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }

        // Auto-save: export readable log + JSON snapshot on exit
        {
            let ch = self.ctx.channel.lock().await;
            if !ch.messages.is_empty() {
                match acp_core::store::export_log(&ch).await {
                    Ok(path) => {
                        tracing::info!(path = %path.display(), "session log saved");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to save session log");
                    }
                }
                match acp_core::store::save(&ch).await {
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to save snapshot");
                    }
                }
            }
        }

        // Cleanup: force-kill all agents immediately
        {
            let clients = self.ctx.clients.lock().await;
            for (_, client) in clients.iter() {
                if let Ok(c) = client.try_lock() {
                    c.force_kill();
                }
            }
        }

        // Remove bus socket
        if let Some(ref path) = self.ctx.socket_path {
            let _ = std::fs::remove_file(path);
        }

        // Clean up leftover .acp-paste-* temp files
        image::cleanup_all_paste_files(&self.cwd).await;

        Ok(())
    }

    async fn handle_bus_event(&self, event: BusEvent) {
        bus_events::handle_bus_event(&self.ctx, event, &self.bus_tx).await;
    }

    async fn update_completions(&mut self) {
        if let Ok(ch) = self.ctx.channel.try_lock() {
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
            context_tokens: None,
        });
        self.messages.streaming.clear();
        self.messages.thinking.clear();

        let ch = self.ctx.channel.lock().await;
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
                tool_calls: agent
                    .tool_calls
                    .iter()
                    .map(|tc| ToolCallDisplay {
                        name: tc.name.clone(),
                        running: tc.status == acp_core::agent::ToolCallStatus::Running,
                    })
                    .collect(),
                context_tokens: agent.context_tokens,
            });
            if agent.streaming && !agent.stream_buf.is_empty() {
                self.messages.streaming.push((
                    agent.name.clone(),
                    agent.stream_buf.clone(),
                    agent
                        .prompt_start_time
                        .map(|t| (chrono::Utc::now().timestamp() - t).max(0)),
                ));
            }
            if !agent.thinking_buf.is_empty() {
                self.messages
                    .thinking
                    .push((agent.name.clone(), agent.thinking_buf.clone()));
            }
        }
        drop(ch);

        // Sync selected agent status to input box
        // Sync input status based on sidebar mode + selection
        {
            use crate::components::status_bar::SidebarMode;
            match self.status_bar.mode {
                SidebarMode::Agents => {
                    if let Some(agent) = self.cached_agents.get(self.status_bar.selected) {
                        let is_system = agent.name == "System";
                        self.input.agent_name = if is_system {
                            None
                        } else {
                            Some(agent.name.clone())
                        };
                        self.input.agent_status = Some(agent.status.clone());
                        self.input.agent_activity = agent.activity.clone();
                        self.input.active_secs = agent
                            .prompt_start_time
                            .map(|t| (chrono::Utc::now().timestamp() - t).max(0));
                    }
                    self.messages.group_members = None;
                }
                SidebarMode::Groups => {
                    if let Some(group) = self.cached_groups.get(self.status_bar.selected) {
                        self.input.agent_name = Some(format!("群组 {}", group.name));
                        self.messages.group_members = Some(group.members.clone());
                    } else {
                        self.input.agent_name = None;
                        self.messages.group_members = None;
                    }
                    self.input.agent_status = None;
                    self.input.agent_activity = None;
                    self.input.active_secs = None;
                }
            }
        }

        // Collect group displays
        {
            use crate::components::status_bar::GroupDisplay;
            let ch = self.ctx.channel.lock().await;
            self.cached_groups = ch
                .groups
                .iter()
                .map(|(name, g)| GroupDisplay {
                    name: name.clone(),
                    member_count: g.members.len(),
                    members: g.members.iter().cloned().collect(),
                })
                .collect();
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        // Compute text area width for input wrapping: total - sidebar - prompt - borders
        let area = frame.area();
        let sidebar_w: u16 = if self.sidebar_collapsed {
            0
        } else if area.width > 100 {
            24
        } else if area.width > 60 {
            20
        } else {
            16
        };
        let input_text_w = area.width.saturating_sub(sidebar_w + 3); // 2 for prompt + 1 for border
        let layout = AppLayout::new(
            area,
            self.input.visual_line_count(input_text_w),
            self.sidebar_collapsed,
        );

        // Sidebar (agent list + groups)
        if let Some(sidebar_area) = layout.sidebar {
            self.status_bar.render(
                &self.cached_agents,
                &self.cached_groups,
                &self.cwd,
                sidebar_area,
                frame.buffer_mut(),
            );
        }

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
                self.cancel_selected_agent().await;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('b')) => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
            }
            (_, KeyCode::Tab) => {
                self.status_bar.toggle_mode();
                self.update_message_filter().await;
            }
            (_, KeyCode::Esc) => {
                self.input.dismiss_popup();
            }
            (KeyModifiers::CONTROL, KeyCode::Enter) => {
                self.input.insert('\n');
            }
            (_, KeyCode::Enter) => {
                // If popup active, confirm selection instead of sending
                if self.input.confirm_selection() {
                    return;
                }
                let has_images = !self.pending_images.images.is_empty();
                if !self.input.is_empty() || has_images {
                    let text = self.input.take();
                    let images = std::mem::take(&mut self.pending_images);
                    self.handle_input(text, images).await;
                }
            }
            (_, KeyCode::Backspace) => self.input.backspace(),
            (_, KeyCode::Delete) => self.input.delete(),
            (_, KeyCode::Home) => self.input.move_home(),
            (_, KeyCode::End) => self.input.move_end(),
            (KeyModifiers::CONTROL, KeyCode::Char('v')) => {
                self.try_paste_image().await;
            }
            (KeyModifiers::CONTROL, KeyCode::Char('j')) => self.messages.scroll_down(1),
            (KeyModifiers::CONTROL, KeyCode::Char('k')) => self.messages.scroll_up(1),
            (KeyModifiers::CONTROL, KeyCode::Char('d')) => self.messages.scroll_down(10),
            (KeyModifiers::CONTROL, KeyCode::Char('u')) => self.messages.scroll_up(10),
            (_, KeyCode::PageDown) => self.messages.scroll_down(20),
            (_, KeyCode::PageUp) => self.messages.scroll_up(20),
            // Ctrl+N/P: popup navigation when active, otherwise tab switching
            (KeyModifiers::CONTROL, KeyCode::Char('n')) | (KeyModifiers::SHIFT, KeyCode::Right) => {
                if self.input.popup_active() {
                    self.input.select_next();
                } else {
                    use crate::components::status_bar::SidebarMode;
                    let count = match self.status_bar.mode {
                        SidebarMode::Agents => self.cached_agents.len(),
                        SidebarMode::Groups => self.cached_groups.len(),
                    };
                    self.status_bar.select_next(count);
                    self.update_message_filter().await;
                }
            }
            (KeyModifiers::CONTROL, KeyCode::Char('p')) | (KeyModifiers::SHIFT, KeyCode::Left) => {
                if self.input.popup_active() {
                    self.input.select_prev();
                } else {
                    use crate::components::status_bar::SidebarMode;
                    let count = match self.status_bar.mode {
                        SidebarMode::Agents => self.cached_agents.len(),
                        SidebarMode::Groups => self.cached_groups.len(),
                    };
                    self.status_bar.select_prev(count);
                    self.update_message_filter().await;
                }
            }
            (_, KeyCode::Left) => self.input.move_left(),
            (_, KeyCode::Right) => self.input.move_right(),
            (_, KeyCode::Char(c)) => self.input.insert(c),
            _ => {}
        }
    }

    /// Returns the agent name for the currently selected tab, or None if "All" is selected.
    async fn selected_agent_name(&self) -> Option<String> {
        let idx = self.status_bar.selected;
        if idx == 0 {
            return None;
        }
        let ch = self.ctx.channel.lock().await;
        let names: Vec<String> = ch.agents.keys().cloned().collect();
        names.get(idx - 1).cloned()
    }

    async fn update_message_filter(&mut self) {
        use crate::components::status_bar::SidebarMode;
        let idx = self.status_bar.selected;

        match self.status_bar.mode {
            SidebarMode::Agents => {
                if idx == 0 {
                    self.messages.filter = None; // System tab
                } else {
                    self.messages.filter = self.cached_agents.get(idx).map(|a| a.name.clone());
                }
                self.messages.group_members = None;
            }
            SidebarMode::Groups => {
                if let Some(group) = self.cached_groups.get(idx) {
                    self.messages.filter = Some(format!("group:{}", group.name));
                    self.messages.group_members = Some(group.members.clone());
                } else {
                    self.messages.filter = None;
                    self.messages.group_members = None;
                }
            }
        }
        self.messages.snap_to_bottom();
    }

    async fn cancel_selected_agent(&self) {
        let name = match self.selected_agent_name().await {
            Some(n) => n,
            None => {
                // System tab: cancel all running agents
                let clients = self.ctx.clients.lock().await;
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
                    let mut ch = self.ctx.channel.lock().await;
                    ch.post("系统", &format!("已中断: {}", cancelled.join(", ")), true);
                }
                return;
            }
        };

        let clients = self.ctx.clients.lock().await;
        if let Some(client) = clients.get(&name) {
            if let Ok(c) = client.try_lock() {
                c.cancel().await;
                let mut ch = self.ctx.channel.lock().await;
                ch.post("系统", &format!("已中断 {name}"), true);
            }
        }
    }

    /// Try to read image from system clipboard, append to pending list,
    /// and insert `[Image-N]` marker in the input box.
    async fn try_paste_image(&mut self) {
        let image = clipboard::read_clipboard_image().await;
        match image {
            Some(img) => {
                self.pending_images.images.push(img);
                let n = self.pending_images.images.len();
                let marker = format!("[Image-{n}]");
                self.input.insert_str(&marker);
            }
            None => {
                let mut ch = self.ctx.channel.lock().await;
                ch.post("系统", "剪贴板中未找到图片", true);
            }
        }
    }

    async fn handle_input(&mut self, text: String, images: PendingImages) {
        if text.starts_with('/') {
            self.handle_command(&text).await;
            return;
        }

        let has_images = !images.images.is_empty();
        // Build display text for channel messages
        let display_text = if has_images {
            let img_tags: String = (1..=images.images.len())
                .map(|i| format!("[Image-{i}]"))
                .collect::<Vec<_>>()
                .join(" ");
            if text.is_empty() {
                img_tags
            } else {
                // Strip [Image-N] markers from text for display, keep user's actual text
                let clean: String = text
                    .split("[Image-")
                    .enumerate()
                    .map(|(i, s)| {
                        if i == 0 {
                            s.to_string()
                        } else {
                            s.splitn(2, ']').last().unwrap_or("").to_string()
                        }
                    })
                    .collect::<String>()
                    .trim()
                    .to_string();
                if clean.is_empty() {
                    img_tags
                } else {
                    format!("{img_tags} {clean}")
                }
            }
        } else {
            text.clone()
        };

        // Convert images for prompting (pass first image)
        let attached_img = if has_images {
            Some(images.images.into_iter().next().unwrap())
        } else {
            None
        };

        // Group mode: send as group message with sequential dispatch
        if self.status_bar.mode == crate::components::status_bar::SidebarMode::Groups {
            if let Some(group) = self.cached_groups.get(self.status_bar.selected) {
                let group_name = group.name.clone();
                let (recipients, history) = {
                    let mut ch = self.ctx.channel.lock().await;
                    let r = ch.group_recipients(&group_name, "you");
                    ch.post_group(&group_name, "you", &display_text);
                    let h = ch.group_history(&group_name, 10);
                    (r, h)
                };
                self.messages.snap_to_bottom();
                let ctx = self.ctx.clone();
                tokio::spawn(dispatch_group_sequential(
                    recipients,
                    history,
                    group_name,
                    "user".to_string(),
                    text,
                    ctx,
                    1,
                ));
            }
            return;
        }

        // Determine target agent
        let has_mention = text.contains('@');
        if has_mention {
            let route_info = {
                let mut ch = self.ctx.channel.lock().await;
                let route_info = ch.post_message(
                    "you",
                    None,
                    &display_text,
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
                entry.content = Some(display_text.clone());
                entry.detail = Some("user broadcast with mentions".to_string());
                let cwd = ch.cwd.clone();
                let channel_id = ch.channel_id.clone();
                drop(ch);
                entry.channel_id = channel_id;
                let _ = acp_core::comm_log::append(&cwd, &entry).await;
                route_info
            };
            if let Some((content, from)) = route_info {
                self.dispatch_to_agents(&content, &from, attached_img).await;
            }
        } else {
            let target = self
                .selected_agent_name()
                .await
                .unwrap_or_else(|| "main".to_string());
            {
                let mut ch = self.ctx.channel.lock().await;
                let message_id = ch.post_directed(
                    "you",
                    &target,
                    &display_text,
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
                entry.content = Some(display_text.clone());
                entry.detail = Some("user direct chat".to_string());
                let _ = acp_core::comm_log::append(&ch.cwd, &entry).await;
            }
            self.dispatch_single_agent(&target, &text, attached_img).await;
        }
    }

    async fn dispatch_to_agents(
        &self,
        content: &str,
        from: &str,
        pending_img: Option<PendingImage>,
    ) {
        let targets = {
            let ch = self.ctx.channel.lock().await;
            let names: Vec<String> = ch.agents.keys().cloned().collect();
            router::route(content, from, &names, 0)
        };

        for (i, target) in targets.iter().enumerate() {
            let name = target.name.clone();
            let content = if from == "you" || from == "你" {
                format!(
                    "[Direct message from user. Reply directly, do not @main.]\n{}",
                    target.content
                )
            } else {
                format!("[Message from {from}]\n{}", target.content)
            };
            let ctx = self.ctx.clone();
            let img = if i == 0 { pending_img.clone() } else { None };

            tokio::spawn(async move {
                prompting::do_prompt_with_image(name, content, ctx, img).await;
            });
        }
    }

    async fn dispatch_single_agent(
        &self,
        name: &str,
        content: &str,
        pending_img: Option<PendingImage>,
    ) {
        let name = name.to_string();
        let content = content.to_string();
        let ctx = self.ctx.clone();

        tokio::spawn(async move {
            prompting::do_prompt_with_image(name, content, ctx, pending_img).await;
        });
    }

    async fn handle_command(&mut self, text: &str) {
        match commands::handle_command(
            text,
            &self.ctx,
            &self.bus_tx,
            &self.cwd,
            &self.default_adapter,
        )
        .await
        {
            commands::CommandResult::Ok => {}
            commands::CommandResult::Quit => {
                self.should_quit = true;
            }
            commands::CommandResult::Error(msg) => {
                let mut ch = self.ctx.channel.lock().await;
                ch.post("系统", &msg, true);
            }
        }
    }

    async fn start_agent(&self, name: String, adapter_name: String) {
        let is_main = is_main_instance(&name);
        lifecycle::start_agent(
            name,
            adapter_name,
            is_main,
            self.ctx.clone(),
            Some(self.bus_tx.clone()),
        )
        .await;
    }
}
