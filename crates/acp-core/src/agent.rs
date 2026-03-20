use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentKind {
    Local,
    Spawned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatus {
    Connecting,
    Idle,
    Streaming,
    Disconnected,
    Error,
}

impl fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentStatus::Connecting => write!(f, "connecting"),
            AgentStatus::Idle => write!(f, "idle"),
            AgentStatus::Streaming => write!(f, "streaming"),
            AgentStatus::Disconnected => write!(f, "disconnected"),
            AgentStatus::Error => write!(f, "error"),
        }
    }
}

/// A tool call record for sidebar display
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub status: ToolCallStatus,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolCallStatus {
    Running,
    Done,
}

const MAX_TOOL_HISTORY: usize = 5;

#[derive(Debug, Clone)]
pub struct Agent {
    pub name: String,
    pub kind: AgentKind,
    pub adapter_name: String,
    pub status: AgentStatus,
    pub activity: Option<String>,
    pub streaming: bool,
    pub stream_buf: String,
    pub system_prompt: Option<String>,
    pub prompted: bool,
    pub session_id: Option<String>,
    pub alive: bool,
    pub last_rpc_time: Option<i64>,
    pub prompt_start_time: Option<i64>,
    pub waiting_reply_from: Option<String>,
    pub waiting_since: Option<i64>,
    pub waiting_conversation_id: Option<u64>,
    pub last_closed_conversation_id: Option<u64>,
    /// Recent tool calls (newest first, max 5)
    pub tool_calls: Vec<ToolCall>,
}

impl Agent {
    pub fn new_spawned(name: String, adapter_name: String, system_prompt: Option<String>) -> Self {
        Self {
            name,
            kind: AgentKind::Spawned,
            adapter_name,
            status: AgentStatus::Connecting,
            activity: None,
            streaming: false,
            stream_buf: String::new(),
            system_prompt,
            prompted: false,
            session_id: None,
            alive: false,
            last_rpc_time: None,
            prompt_start_time: None,
            waiting_reply_from: None,
            waiting_since: None,
            waiting_conversation_id: None,
            last_closed_conversation_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn new_local() -> Self {
        Self {
            name: "main".to_string(),
            kind: AgentKind::Local,
            adapter_name: "local".to_string(),
            status: AgentStatus::Idle,
            activity: None,
            streaming: false,
            stream_buf: String::new(),
            system_prompt: None,
            prompted: false,
            session_id: None,
            alive: false,
            last_rpc_time: None,
            prompt_start_time: None,
            waiting_reply_from: None,
            waiting_since: None,
            waiting_conversation_id: None,
            last_closed_conversation_id: None,
            tool_calls: Vec::new(),
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn push_tool_call(&mut self, name: String) {
        // Mark previous running tool as done
        if let Some(last) = self.tool_calls.first_mut() {
            last.status = ToolCallStatus::Done;
        }
        self.tool_calls.insert(0, ToolCall {
            name,
            status: ToolCallStatus::Running,
        });
        if self.tool_calls.len() > MAX_TOOL_HISTORY {
            self.tool_calls.truncate(MAX_TOOL_HISTORY);
        }
    }

    pub fn finish_tool_calls(&mut self) {
        for tc in &mut self.tool_calls {
            tc.status = ToolCallStatus::Done;
        }
    }

    pub fn reset_stream(&mut self) {
        self.streaming = false;
        self.stream_buf.clear();
        self.activity = None;
    }
}
