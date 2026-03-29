use tokio::sync::oneshot;

/// Events emitted by agents via the bus socket, handled by the TUI event loop.
#[derive(Debug)]
pub enum BusEvent {
    SendMessage {
        from_agent: String,
        to_agent: String,
        content: String,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    ListAgents {
        from_agent: String,
        reply_tx: oneshot::Sender<Vec<AgentInfo>>,
    },
    CreateAgent {
        from_agent: String,
        name: String,
        adapter: String,
        task: Option<String>,
        reply_tx: oneshot::Sender<CreateAgentResult>,
    },
    RemoveAgent {
        from_agent: String,
        name: String,
        reply_tx: oneshot::Sender<RemoveAgentResult>,
    },
    SendAndWait {
        from_agent: String,
        to_agent: String,
        content: String,
        timeout_secs: u64,
        reply_tx: oneshot::Sender<SendAndWaitResult>,
    },
    Reply {
        from_agent: String,
        to_agent: String,
        content: String,
        in_reply_to: Option<u64>,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    CreateGroup {
        from_agent: String,
        name: String,
        members: Vec<String>,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    GroupMessage {
        from_agent: String,
        group_name: String,
        content: String,
        rounds: u32,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    GroupAdd {
        from_agent: String,
        group_name: String,
        member: String,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
}

#[derive(Debug, Clone)]
pub struct BusSendResult {
    pub message_id: Option<u64>,
    pub delivered: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateAgentResult {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RemoveAgentResult {
    pub ok: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SendAndWaitResult {
    pub ok: bool,
    pub reply_content: Option<String>,
    pub from_agent: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub status: String,
    pub adapter: String,
    pub activity: Option<String>,
    pub active_secs: Option<i64>,
    pub current_task: Option<String>,
    pub inbox_depth: usize,
    pub waiting_for: Option<String>,
}
