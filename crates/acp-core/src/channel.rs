use std::collections::HashMap;

use indexmap::IndexMap;

use chrono::Utc;
use tokio::sync::broadcast;

use crate::group::Group;

use crate::agent::{Agent, AgentKind};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MessageKind {
    Chat,
    Task,
    System,
    Audit,
}

impl MessageKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Task => "task",
            Self::System => "system",
            Self::Audit => "audit",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MessageTransport {
    Ui,
    MentionRoute,
    BusTool,
    Internal,
}

impl MessageTransport {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::MentionRoute => "mention",
            Self::BusTool => "bus",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum MessageStatus {
    Queued,
    Sent,
    Delivered,
    Failed,
}

impl MessageStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Sent => "sent",
            Self::Delivered => "delivered",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum SystemKind {
    AgentOnline,
    AgentOffline,
    AgentComplete,
    AgentError,
    QueueNotice,
    Routing,
    General,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Message {
    pub id: u64,
    pub conversation_id: u64,
    pub reply_to: Option<u64>,
    pub from: String,
    /// Target agent (for directed messages). None = broadcast.
    pub to: Option<String>,
    pub content: String,
    pub kind: MessageKind,
    pub transport: MessageTransport,
    pub status: MessageStatus,
    pub error: Option<String>,
    pub timestamp: i64,
    #[serde(default)]
    pub system_kind: Option<SystemKind>,
    /// Group name if this is a group message.
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone)]
pub enum ChannelEvent {
    NewMessage { message: Message, gap: Option<i64> },
    StateChanged,
    Closed,
}

pub struct Channel {
    pub channel_id: String,
    pub cwd: String,
    pub messages: Vec<Message>,
    pub agents: IndexMap<String, Agent>,
    pub groups: HashMap<String, Group>,
    event_tx: broadcast::Sender<ChannelEvent>,
    last_msg_time: Option<i64>,
    saved: bool,
    next_msg_id: u64,
}

impl Channel {
    pub fn new(cwd: String) -> Self {
        let channel_id = Utc::now().format("%Y%m%d-%H%M%S").to_string();
        let (event_tx, _) = broadcast::channel(256);
        let mut channel = Self {
            channel_id,
            cwd,
            messages: Vec::new(),
            agents: IndexMap::new(),
            groups: HashMap::new(),
            event_tx,
            last_msg_time: None,
            saved: false,
            next_msg_id: 1,
        };
        // Register main agent
        channel
            .agents
            .insert("main".to_string(), Agent::new_local());
        channel
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChannelEvent> {
        self.event_tx.subscribe()
    }

    /// Post a message directed at a specific agent.
    pub fn post_to(
        &mut self,
        from: &str,
        to: &str,
        content: &str,
        no_route: bool,
    ) -> Option<(String, String)> {
        self.post_message(
            from,
            Some(to.to_string()),
            content,
            MessageKind::Chat,
            MessageTransport::Ui,
            MessageStatus::Sent,
            None,
            no_route,
        )
    }

    /// Post a message to the channel (broadcast).
    pub fn post(&mut self, from: &str, content: &str, no_route: bool) -> Option<(String, String)> {
        self.post_message(
            from,
            None,
            content,
            if from == "系统" {
                MessageKind::System
            } else {
                MessageKind::Chat
            },
            if from == "系统" {
                MessageTransport::Internal
            } else {
                MessageTransport::Ui
            },
            MessageStatus::Sent,
            None,
            no_route,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_message(
        &mut self,
        from: &str,
        to: Option<String>,
        content: &str,
        kind: MessageKind,
        transport: MessageTransport,
        status: MessageStatus,
        error: Option<String>,
        no_route: bool,
    ) -> Option<(String, String)> {
        self.post_message_with_refs(
            from, to, content, kind, transport, status, None, None, error, no_route,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_message_with_refs(
        &mut self,
        from: &str,
        to: Option<String>,
        content: &str,
        kind: MessageKind,
        transport: MessageTransport,
        status: MessageStatus,
        conversation_id: Option<u64>,
        reply_to: Option<u64>,
        error: Option<String>,
        no_route: bool,
    ) -> Option<(String, String)> {
        let content = content.to_string();

        let now = Utc::now().timestamp();
        let gap = self.last_msg_time.map(|t| now - t);
        self.last_msg_time = Some(now);

        // Update agent's last_rpc_time
        if let Some(agent) = self.agents.get_mut(from) {
            if agent.kind != AgentKind::Local {
                agent.last_rpc_time = Some(now);
            }
        }

        let msg = Message {
            id: self.next_msg_id,
            conversation_id: conversation_id.unwrap_or(self.next_msg_id),
            reply_to,
            from: from.to_string(),
            to,
            content: content.clone(),
            kind,
            transport,
            status,
            error,
            timestamp: now,
            system_kind: None,
            group: None,
        };
        self.next_msg_id += 1;
        self.messages.push(msg.clone());
        let _ = self
            .event_tx
            .send(ChannelEvent::NewMessage { message: msg, gap });

        if no_route {
            None
        } else {
            // Return content and from for router to process
            Some((content, from.to_string()))
        }
    }

    pub fn post_directed(
        &mut self,
        from: &str,
        to: &str,
        content: &str,
        kind: MessageKind,
        transport: MessageTransport,
        status: MessageStatus,
    ) -> u64 {
        self.post_message(
            from,
            Some(to.to_string()),
            content,
            kind,
            transport,
            status,
            None,
            true,
        );
        self.messages.last().map(|m| m.id).unwrap_or(0)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn post_directed_with_refs(
        &mut self,
        from: &str,
        to: &str,
        content: &str,
        kind: MessageKind,
        transport: MessageTransport,
        status: MessageStatus,
        conversation_id: Option<u64>,
        reply_to: Option<u64>,
    ) -> u64 {
        self.post_message_with_refs(
            from,
            Some(to.to_string()),
            content,
            kind,
            transport,
            status,
            conversation_id,
            reply_to,
            None,
            true,
        );
        self.messages.last().map(|m| m.id).unwrap_or(0)
    }

    pub fn mark_waiting(&mut self, agent_name: &str, target: &str, conversation_id: u64) {
        if let Some(agent) = self.agents.get_mut(agent_name) {
            agent.waiting_reply_from = Some(target.to_string());
            agent.waiting_since = Some(Utc::now().timestamp());
            agent.waiting_conversation_id = Some(conversation_id);
        }
    }

    pub fn resolve_reply_context(&mut self, from: &str, to: &str) -> (Option<u64>, Option<u64>) {
        if let Some(agent) = self.agents.get_mut(to) {
            if agent.waiting_reply_from.as_deref() == Some(from) {
                let conversation_id = agent.waiting_conversation_id;
                agent.last_closed_conversation_id = conversation_id;
                agent.waiting_reply_from = None;
                agent.waiting_since = None;
                agent.waiting_conversation_id = None;
                return (conversation_id, conversation_id);
            }
        }
        (None, None)
    }

    pub fn post_system(&mut self, content: &str) -> u64 {
        self.post_message(
            "系统",
            None,
            content,
            MessageKind::System,
            MessageTransport::Internal,
            MessageStatus::Sent,
            None,
            true,
        );
        self.messages.last().map(|m| m.id).unwrap_or(0)
    }

    pub fn post_system_typed(&mut self, content: &str, sys_kind: SystemKind) -> u64 {
        let id = self.post_system(content);
        if let Some(msg) = self.messages.last_mut() {
            msg.system_kind = Some(sys_kind);
        }
        id
    }

    pub fn post_audit(&mut self, content: &str) -> u64 {
        self.post_message(
            "系统",
            None,
            content,
            MessageKind::Audit,
            MessageTransport::Internal,
            MessageStatus::Sent,
            None,
            true,
        );
        self.messages.last().map(|m| m.id).unwrap_or(0)
    }

    pub fn state_changed(&self) {
        let _ = self.event_tx.send(ChannelEvent::StateChanged);
    }

    // --- Group operations ---

    pub fn create_group(&mut self, name: &str, creator: &str) -> bool {
        if self.groups.contains_key(name) {
            return false;
        }
        self.groups
            .insert(name.to_string(), Group::new(name, creator));
        self.post_system_typed(
            &format!("群组 [{name}] 已创建，创建者: {creator}"),
            SystemKind::General,
        );
        self.state_changed();
        true
    }

    /// Post a single message visible to all group members.
    /// Constructs the message with `group` set from the start so the broadcast
    /// event carries the field (fixes group tab filtering).
    pub fn post_group(&mut self, group_name: &str, from: &str, content: &str) -> u64 {
        let content = content.to_string();
        let now = Utc::now().timestamp();
        let gap = self.last_msg_time.map(|t| now - t);
        self.last_msg_time = Some(now);

        if let Some(agent) = self.agents.get_mut(from) {
            if agent.kind != AgentKind::Local {
                agent.last_rpc_time = Some(now);
            }
        }

        let id = self.next_msg_id;
        let msg = Message {
            id,
            conversation_id: id,
            reply_to: None,
            from: from.to_string(),
            to: None,
            content,
            kind: MessageKind::Chat,
            transport: MessageTransport::BusTool,
            status: MessageStatus::Delivered,
            error: None,
            timestamp: now,
            system_kind: None,
            group: Some(group_name.to_string()),
        };
        self.next_msg_id += 1;
        self.messages.push(msg.clone());
        let _ = self
            .event_tx
            .send(ChannelEvent::NewMessage { message: msg, gap });
        id
    }

    /// Return the last `limit` group messages as (from, content) pairs in chronological order.
    pub fn group_history(&self, group_name: &str, limit: usize) -> Vec<(String, String)> {
        let mut items: Vec<(String, String)> = self
            .messages
            .iter()
            .rev()
            .filter(|m| m.group.as_deref() == Some(group_name))
            .take(limit)
            .map(|m| (m.from.clone(), m.content.clone()))
            .collect();
        items.reverse();
        items
    }

    pub fn group_recipients(&self, group_name: &str, sender: &str) -> Vec<String> {
        self.groups
            .get(group_name)
            .map(|g| g.other_members(sender).into_iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Read last N messages.
    pub fn read(&self, last_n: usize) -> &[Message] {
        let total = self.messages.len();
        let start = total.saturating_sub(last_n);
        &self.messages[start..]
    }

    /// List all agents with their status.
    pub fn list_agents(&self) -> Vec<AgentInfo> {
        self.agents
            .iter()
            .map(|(name, agent)| AgentInfo {
                name: name.clone(),
                kind: format!("{:?}", agent.kind).to_lowercase(),
                status: agent.status.to_string(),
                alive: agent.is_alive(),
            })
            .collect()
    }

    /// Remove an agent from the channel.
    pub fn remove_agent(&mut self, name: &str) -> Option<Agent> {
        let agent = self.agents.shift_remove(name)?;
        self.post_system_typed(&format!("{name} 已退出频道"), SystemKind::AgentOffline);
        self.state_changed();
        Some(agent)
    }

    /// Mark channel as saved.
    pub fn mark_saved(&mut self) {
        self.saved = true;
    }

    pub fn is_saved(&self) -> bool {
        self.saved
    }

    /// Close the channel: cleanup non-main agents.
    pub fn close(&mut self) {
        let main = self.agents.shift_remove("main");
        self.agents.clear();
        if let Some(main) = main {
            self.agents.insert("main".to_string(), main);
        }
        let _ = self.event_tx.send(ChannelEvent::Closed);
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub kind: String,
    pub status: String,
    pub alive: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_channel_has_main() {
        let ch = Channel::new("/tmp".into());
        assert!(ch.agents.contains_key("main"));
        assert_eq!(ch.agents.len(), 1);
        assert_eq!(ch.agents["main"].kind, AgentKind::Local);
    }

    #[tokio::test]
    async fn test_post_broadcast() {
        let mut ch = Channel::new("/tmp".into());
        let mut rx = ch.subscribe();

        let result = ch.post("user", "hello world", false);

        assert!(result.is_some());
        assert_eq!(ch.messages.len(), 1);
        let msg = &ch.messages[0];
        assert_eq!(msg.id, 1);
        assert_eq!(msg.from, "user");
        assert!(msg.to.is_none());
        assert_eq!(msg.content, "hello world");
        assert_eq!(msg.kind, MessageKind::Chat);
        assert_eq!(msg.transport, MessageTransport::Ui);

        let event = rx.try_recv().unwrap();
        match event {
            ChannelEvent::NewMessage { message, .. } => {
                assert_eq!(message.from, "user");
                assert_eq!(message.content, "hello world");
            }
            _ => panic!("expected NewMessage event"),
        }
    }

    #[test]
    fn test_post_to_directed() {
        let mut ch = Channel::new("/tmp".into());
        ch.post_to("user", "worker1", "do stuff", true);

        assert_eq!(ch.messages.len(), 1);
        let msg = &ch.messages[0];
        assert_eq!(msg.from, "user");
        assert_eq!(msg.to.as_deref(), Some("worker1"));
        assert_eq!(msg.content, "do stuff");
        assert_eq!(msg.kind, MessageKind::Chat);
    }

    #[test]
    fn test_post_returns_route_info() {
        let mut ch = Channel::new("/tmp".into());
        let result = ch.post("alice", "hey @bob", false);

        assert!(result.is_some());
        let (content, from) = result.unwrap();
        assert_eq!(content, "hey @bob");
        assert_eq!(from, "alice");
    }

    #[test]
    fn test_post_no_route() {
        let mut ch = Channel::new("/tmp".into());
        let result = ch.post("alice", "internal msg", true);
        assert!(result.is_none());
    }

    #[test]
    fn test_post_audit_message() {
        let mut ch = Channel::new("/tmp".into());
        let id = ch.post_audit("w1 -> w2 delivered");
        let msg = ch.messages.last().unwrap();
        assert_eq!(id, msg.id);
        assert_eq!(msg.kind, MessageKind::Audit);
        assert_eq!(msg.transport, MessageTransport::Internal);
    }

    #[test]
    fn test_remove_agent() {
        let mut ch = Channel::new("/tmp".into());
        ch.agents.insert(
            "w1".to_string(),
            Agent::new_spawned("w1".into(), "claude".into(), None),
        );
        ch.agents.insert(
            "w2".to_string(),
            Agent::new_spawned("w2".into(), "claude".into(), None),
        );

        // Order: main, w1, w2
        let keys: Vec<_> = ch.agents.keys().cloned().collect();
        assert_eq!(keys, vec!["main", "w1", "w2"]);

        let removed = ch.remove_agent("w1");
        assert!(removed.is_some());

        // After shift_remove, order preserved: main, w2
        let keys: Vec<_> = ch.agents.keys().cloned().collect();
        assert_eq!(keys, vec!["main", "w2"]);
    }

    #[test]
    fn test_system_kind_on_typed_message() {
        let mut ch = Channel::new("/tmp".into());
        ch.post_system_typed("main 已上线", SystemKind::AgentOnline);
        let msg = ch.messages.last().unwrap();
        assert_eq!(msg.kind, MessageKind::System);
        assert_eq!(msg.system_kind, Some(SystemKind::AgentOnline));
    }

    #[test]
    fn test_system_kind_none_on_regular_system() {
        let mut ch = Channel::new("/tmp".into());
        ch.post_system("普通系统消息");
        let msg = ch.messages.last().unwrap();
        assert_eq!(msg.system_kind, None);
    }

    #[test]
    fn test_system_kind_queue_notice() {
        let mut ch = Channel::new("/tmp".into());
        ch.post_system_typed("消息已排队", SystemKind::QueueNotice);
        let msg = ch.messages.last().unwrap();
        assert_eq!(msg.system_kind, Some(SystemKind::QueueNotice));
    }

    #[test]
    fn test_create_group() {
        let mut ch = Channel::new("/tmp".into());
        assert!(ch.create_group("research", "main"));
        assert!(ch.groups.contains_key("research"));
        assert!(!ch.create_group("research", "main")); // duplicate
    }

    #[test]
    fn test_group_recipients() {
        let mut ch = Channel::new("/tmp".into());
        ch.create_group("team", "main");
        ch.groups.get_mut("team").unwrap().add_member("w1");
        ch.groups.get_mut("team").unwrap().add_member("w2");
        let recipients = ch.group_recipients("team", "main");
        assert!(recipients.contains(&"w1".to_string()));
        assert!(recipients.contains(&"w2".to_string()));
        assert!(!recipients.contains(&"main".to_string()));
    }

    #[test]
    fn test_group_recipients_empty() {
        let ch = Channel::new("/tmp".into());
        assert!(ch.group_recipients("nonexistent", "main").is_empty());
    }

    #[test]
    fn test_post_group_creates_single_message() {
        let mut ch = Channel::new("/tmp".into());
        ch.create_group("team", "main");
        ch.groups.get_mut("team").unwrap().add_member("alice");
        ch.groups.get_mut("team").unwrap().add_member("bob");

        let before = ch.messages.len();
        ch.post_group("team", "alice", "大家好");
        // Should create exactly ONE message, not N
        assert_eq!(ch.messages.len(), before + 1);
        let msg = ch.messages.last().unwrap();
        assert_eq!(msg.group, Some("team".to_string()));
        assert_eq!(msg.from, "alice");
        assert_eq!(msg.to, None); // broadcast within group
        assert!(msg.content.contains("大家好"));
    }

    #[tokio::test]
    async fn test_post_group_event_carries_group_field() {
        let mut ch = Channel::new("/tmp".into());
        let mut rx = ch.subscribe();
        ch.create_group("team", "main");

        ch.post_group("team", "alice", "hello group");

        let event = rx.try_recv().expect("should receive group creation event");
        // Skip the system message from create_group
        match event {
            ChannelEvent::NewMessage { .. } => {}
            _ => panic!("expected NewMessage"),
        }
        let _ = rx.try_recv(); // StateChanged from create_group

        let event = rx.try_recv().expect("should receive group message event");
        match event {
            ChannelEvent::NewMessage { message, .. } => {
                assert_eq!(
                    message.group,
                    Some("team".to_string()),
                    "event message must carry group field, got: {:?}",
                    message.group
                );
                assert_eq!(message.from, "alice");
                assert_eq!(message.content, "hello group");
            }
            _ => panic!("expected NewMessage event"),
        }
    }

    #[test]
    fn test_group_history_returns_recent_messages() {
        let mut ch = Channel::new("/tmp".into());
        ch.create_group("team", "main");
        ch.post_group("team", "alice", "msg1");
        ch.post_group("team", "bob", "msg2");
        ch.post_group("team", "alice", "msg3");
        ch.post("you", "dm message", true); // not a group message

        let history = ch.group_history("team", 10);
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].0, "alice");
        assert_eq!(history[0].1, "msg1");
        assert_eq!(history[2].0, "alice");
        assert_eq!(history[2].1, "msg3");
    }

    #[test]
    fn test_group_history_respects_limit() {
        let mut ch = Channel::new("/tmp".into());
        ch.create_group("team", "main");
        for i in 0..5 {
            ch.post_group("team", "alice", &format!("msg{i}"));
        }
        let history = ch.group_history("team", 2);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].1, "msg3");
        assert_eq!(history[1].1, "msg4");
    }

    #[test]
    fn test_group_history_empty() {
        let ch = Channel::new("/tmp".into());
        let history = ch.group_history("nonexistent", 10);
        assert!(history.is_empty());
    }
}
