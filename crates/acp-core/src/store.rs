use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::channel::{Channel, Message, MessageKind, MessageStatus, MessageTransport};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub version: u32,
    pub channel_id: String,
    pub saved_at: String,
    pub cwd: String,
    pub agents: Vec<SnapshotAgent>,
    pub history: Vec<SnapshotMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotAgent {
    pub name: String,
    pub kind: String,
    pub adapter: String,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMessage {
    pub id: u64,
    pub conversation_id: u64,
    pub reply_to: Option<u64>,
    pub from: String,
    pub to: Option<String>,
    pub content: String,
    pub kind: String,
    pub transport: String,
    pub status: String,
    pub timestamp: i64,
}

/// Encode cwd for filesystem-safe directory name.
fn encode_cwd(cwd: &str) -> String {
    cwd.trim_start_matches('/').replace('/', "-")
}

fn storage_dir(cwd: &str) -> PathBuf {
    let base = dirs_or_default();
    base.join("acp-bus/channels").join(encode_cwd(cwd))
}

fn dirs_or_default() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Save a channel snapshot to disk.
pub async fn save(channel: &Channel) -> anyhow::Result<PathBuf> {
    if channel.messages.is_empty() {
        anyhow::bail!("no messages to save");
    }

    let agents: Vec<SnapshotAgent> = channel
        .agents
        .iter()
        .map(|(name, agent)| SnapshotAgent {
            name: name.clone(),
            kind: format!("{:?}", agent.kind).to_lowercase(),
            adapter: agent.adapter_name.clone(),
            session_id: agent.session_id.clone(),
        })
        .collect();

    let history: Vec<SnapshotMessage> = channel
        .messages
        .iter()
        .map(|m| SnapshotMessage {
            id: m.id,
            conversation_id: m.conversation_id,
            reply_to: m.reply_to,
            from: m.from.clone(),
            to: m.to.clone(),
            content: m.content.clone(),
            kind: m.kind.as_str().to_string(),
            transport: m.transport.as_str().to_string(),
            status: m.status.as_str().to_string(),
            timestamp: m.timestamp,
        })
        .collect();

    let snapshot = Snapshot {
        version: 1,
        channel_id: channel.channel_id.clone(),
        saved_at: chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
        cwd: channel.cwd.clone(),
        agents,
        history,
    };

    let dir = storage_dir(&channel.cwd);
    tokio::fs::create_dir_all(&dir).await?;
    let filepath = dir.join(format!("{}.json", channel.channel_id));
    let json = serde_json::to_string_pretty(&snapshot)?;
    tokio::fs::write(&filepath, json).await?;

    info!(path = %filepath.display(), "snapshot saved");
    Ok(filepath)
}

/// List saved snapshots for a given cwd (newest first).
pub async fn list_snapshots(cwd: &str) -> anyhow::Result<Vec<SnapshotInfo>> {
    let dir = storage_dir(cwd);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = tokio::fs::read_dir(&dir).await?;
    let mut result = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            if let Ok(data) = tokio::fs::read_to_string(&path).await {
                if let Ok(snapshot) = serde_json::from_str::<Snapshot>(&data) {
                    let agent_names: Vec<String> = snapshot
                        .agents
                        .iter()
                        .map(|a| format!("{}({})", a.name, a.adapter))
                        .collect();
                    result.push(SnapshotInfo {
                        channel_id: snapshot.channel_id,
                        saved_at: snapshot.saved_at,
                        agents: agent_names.join(", "),
                        msg_count: snapshot.history.len(),
                        filepath: path,
                    });
                }
            }
        }
    }

    result.sort_by(|a, b| b.channel_id.cmp(&a.channel_id));
    Ok(result)
}

/// Load a snapshot from file.
pub async fn load(filepath: &Path) -> anyhow::Result<Snapshot> {
    let data = tokio::fs::read_to_string(filepath).await?;
    let snapshot: Snapshot = serde_json::from_str(&data)?;
    Ok(snapshot)
}

#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    pub channel_id: String,
    pub saved_at: String,
    pub agents: String,
    pub msg_count: usize,
    pub filepath: PathBuf,
}

impl From<&SnapshotMessage> for Message {
    fn from(m: &SnapshotMessage) -> Self {
        Message {
            id: m.id,
            conversation_id: m.conversation_id,
            reply_to: m.reply_to,
            from: m.from.clone(),
            to: m.to.clone(),
            content: m.content.clone(),
            kind: match m.kind.as_str() {
                "task" => MessageKind::Task,
                "system" => MessageKind::System,
                "audit" => MessageKind::Audit,
                _ => MessageKind::Chat,
            },
            transport: match m.transport.as_str() {
                "mention" => MessageTransport::MentionRoute,
                "bus" => MessageTransport::BusTool,
                "internal" => MessageTransport::Internal,
                _ => MessageTransport::Ui,
            },
            status: match m.status.as_str() {
                "queued" => MessageStatus::Queued,
                "delivered" => MessageStatus::Delivered,
                "failed" => MessageStatus::Failed,
                _ => MessageStatus::Sent,
            },
            error: None,
            timestamp: m.timestamp,
        }
    }
}
