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
    /// Accumulated thinking text (from agent_thought_chunk events)
    pub thinking_buf: String,
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
    /// Task pending delivery (stored when agent wasn't connected at dispatch time)
    pub pending_task: Option<String>,
    /// Short description of the current task being processed (first 100 chars)
    pub current_task: Option<String>,
    /// Whether this agent made any bus tool calls during current prompt
    pub has_bus_activity: bool,
    /// Context token usage: (input_tokens, max_context_tokens)
    pub context_tokens: Option<(u64, u64)>,
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
            thinking_buf: String::new(),
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
            pending_task: None,
            current_task: None,
            has_bus_activity: false,
            context_tokens: None,
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
            thinking_buf: String::new(),
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
            pending_task: None,
            current_task: None,
            has_bus_activity: false,
            context_tokens: None,
        }
    }

    pub fn is_alive(&self) -> bool {
        self.alive
    }

    /// Clean MCP tool name for display: strip `mcp__server__` prefixes.
    pub fn clean_tool_name(raw: &str) -> String {
        // Pattern: mcp__<server>__<tool> → just <tool>
        if let Some(rest) = raw.strip_prefix("mcp__") {
            if let Some(pos) = rest.find("__") {
                return rest[pos + 2..].to_string();
            }
        }
        raw.to_string()
    }

    pub fn push_tool_call(&mut self, name: String) {
        // Mark previous running tool as done
        if let Some(last) = self.tool_calls.first_mut() {
            last.status = ToolCallStatus::Done;
        }
        self.tool_calls.insert(
            0,
            ToolCall {
                name,
                status: ToolCallStatus::Running,
            },
        );
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
        self.thinking_buf.clear();
        self.activity = None;
        self.has_bus_activity = false;
        self.waiting_reply_from = None;
        self.waiting_since = None;
        self.waiting_conversation_id = None;
    }

    /// Whether the "(完成，无文本输出)" message should be shown.
    /// Returns false if agent produced text output or communicated via bus tools.
    pub fn should_show_empty_output(&self) -> bool {
        self.stream_buf.is_empty() && !self.has_bus_activity
    }

    /// Whether auto-reply should be suppressed after prompt completion.
    /// True when agent already communicated via bus tools (e.g., bus_reply).
    pub fn should_suppress_auto_reply(&self) -> bool {
        self.has_bus_activity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_spawned_has_bus_activity_false() {
        let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        assert!(!agent.has_bus_activity);
    }

    #[test]
    fn new_local_has_bus_activity_false() {
        let agent = Agent::new_local();
        assert!(!agent.has_bus_activity);
    }

    #[test]
    fn reset_stream_clears_bus_activity() {
        let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        agent.has_bus_activity = true;
        agent.reset_stream();
        assert!(!agent.has_bus_activity);
    }

    #[test]
    fn should_show_empty_output_true_when_no_activity() {
        let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        assert!(agent.should_show_empty_output());
    }

    #[test]
    fn should_show_empty_output_false_when_has_bus_activity() {
        let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        agent.has_bus_activity = true;
        assert!(!agent.should_show_empty_output());
    }

    #[test]
    fn should_show_empty_output_false_when_has_stream_content() {
        let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        agent.stream_buf.push_str("some output");
        assert!(!agent.should_show_empty_output());
    }

    #[test]
    fn new_spawned_has_empty_thinking_buf() {
        let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        assert!(agent.thinking_buf.is_empty());
    }

    #[test]
    fn reset_stream_clears_thinking_buf() {
        let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        agent.thinking_buf.push_str("some thinking");
        agent.reset_stream();
        assert!(agent.thinking_buf.is_empty());
    }

    #[test]
    fn reset_stream_clears_waiting_state() {
        let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
        agent.waiting_reply_from = Some("bob".into());
        agent.waiting_since = Some(12345);
        agent.waiting_conversation_id = Some(42);
        agent.reset_stream();
        assert!(agent.waiting_reply_from.is_none());
        assert!(agent.waiting_since.is_none());
        assert!(agent.waiting_conversation_id.is_none());
    }

    #[test]
    fn clean_tool_name_strips_mcp_prefix() {
        assert_eq!(
            Agent::clean_tool_name("mcp__acp-bus__bus_reply"),
            "bus_reply"
        );
        assert_eq!(
            Agent::clean_tool_name("mcp__acp-bus__bus_send_message"),
            "bus_send_message"
        );
    }

    #[test]
    fn clean_tool_name_keeps_plain_names() {
        assert_eq!(Agent::clean_tool_name("Read"), "Read");
        assert_eq!(Agent::clean_tool_name("Bash"), "Bash");
    }

    #[test]
    fn clean_tool_name_handles_single_prefix() {
        // Only one __, not a full mcp__ prefix
        assert_eq!(Agent::clean_tool_name("some__tool"), "some__tool");
    }

    #[test]
    fn suppress_auto_reply_when_bus_active() {
        let mut agent = Agent::new_spawned("bob".into(), "claude".into(), None);
        assert!(!agent.should_suppress_auto_reply());
        agent.has_bus_activity = true;
        assert!(agent.should_suppress_auto_reply());
    }

    #[test]
    fn no_suppress_when_no_bus_activity() {
        let agent = Agent::new_spawned("bob".into(), "claude".into(), None);
        assert!(!agent.should_suppress_auto_reply());
    }
}
