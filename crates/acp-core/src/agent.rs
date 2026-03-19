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
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    pub fn reset_stream(&mut self) {
        self.streaming = false;
        self.stream_buf.clear();
        self.activity = None;
    }
}
