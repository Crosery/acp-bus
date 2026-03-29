use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

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
/// Uses percent-encoding for `-` to avoid collisions (e.g. "/a/b" vs "/a-b").
fn encode_cwd(cwd: &str) -> String {
    cwd.trim_start_matches('/')
        .replace('%', "%25")
        .replace('-', "%2D")
        .replace('/', "-")
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
    // Atomic write: write to temp file then rename to prevent corruption
    let tmp = filepath.with_extension("json.tmp");
    tokio::fs::write(&tmp, json).await?;
    tokio::fs::rename(&tmp, &filepath).await?;

    info!(path = %filepath.display(), "snapshot saved");
    Ok(filepath)
}

/// Export channel messages as a human-readable Markdown log.
pub async fn export_log(channel: &Channel) -> anyhow::Result<PathBuf> {
    if channel.messages.is_empty() {
        anyhow::bail!("no messages to export");
    }

    let mut md = String::new();
    md.push_str(&format!("# 会话日志 {}\n\n", channel.channel_id));
    md.push_str(&format!("**工作目录**: `{}`\n\n", channel.cwd));

    // Agent list
    let agents: Vec<String> = channel
        .agents
        .iter()
        .filter(|(n, _)| *n != "main")
        .map(|(n, a)| format!("{n} ({})", a.adapter_name))
        .collect();
    if !agents.is_empty() {
        md.push_str(&format!("**Agents**: {}\n\n", agents.join(", ")));
    }
    md.push_str("---\n\n");

    for msg in &channel.messages {
        let ts = chrono::DateTime::from_timestamp(msg.timestamp, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_default();

        let group_tag = msg
            .group
            .as_ref()
            .map(|g| format!(" [{}]", g))
            .unwrap_or_default();

        let direction = msg
            .to
            .as_ref()
            .map(|t| format!(" → {t}"))
            .unwrap_or_default();

        if msg.kind == MessageKind::System || msg.kind == MessageKind::Audit {
            md.push_str(&format!("*{ts} 系统: {}*\n\n", msg.content));
        } else {
            md.push_str(&format!(
                "**{}{direction}{group_tag}** ({ts})\n\n",
                msg.from
            ));
            // Indent content for readability
            for line in msg.content.lines() {
                md.push_str(&format!("{line}\n"));
            }
            md.push('\n');
        }
    }

    let dir = storage_dir(&channel.cwd);
    tokio::fs::create_dir_all(&dir).await?;
    let filepath = dir.join(format!("{}.md", channel.channel_id));
    tokio::fs::write(&filepath, md).await?;

    info!(path = %filepath.display(), "log exported");
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
            let data = match tokio::fs::read_to_string(&path).await {
                Ok(d) => d,
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "failed to read snapshot");
                    continue;
                }
            };
            match serde_json::from_str::<Snapshot>(&data) {
                Ok(snapshot) => {
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
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "corrupt snapshot, skipping");
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
            system_kind: None,
            group: None,
        }
    }
}
