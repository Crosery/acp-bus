use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommLogEntry {
    pub ts: String,
    pub channel_id: String,
    pub event: String,
    pub from: Option<String>,
    pub to: Option<String>,
    pub transport: Option<String>,
    pub status: Option<String>,
    pub message_id: Option<u64>,
    pub conversation_id: Option<u64>,
    pub reply_to: Option<u64>,
    pub content: Option<String>,
    pub detail: Option<String>,
}

fn encode_cwd(cwd: &str) -> String {
    cwd.trim_start_matches('/').replace('/', "-")
}

fn base_dir(cwd: &str) -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("acp-bus/comm-logs").join(encode_cwd(cwd))
}

pub fn log_path(cwd: &str, channel_id: &str) -> PathBuf {
    base_dir(cwd).join(format!("{channel_id}.jsonl"))
}

pub async fn append(cwd: &str, entry: &CommLogEntry) -> anyhow::Result<PathBuf> {
    let path = log_path(cwd, &entry.channel_id);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');

    use tokio::io::AsyncWriteExt;
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await?;
    file.write_all(line.as_bytes()).await?;
    file.flush().await?;
    Ok(path)
}

pub fn entry(channel_id: &str, event: &str) -> CommLogEntry {
    CommLogEntry {
        ts: chrono::Utc::now()
            .format("%Y-%m-%dT%H:%M:%S%.3fZ")
            .to_string(),
        channel_id: channel_id.to_string(),
        event: event.to_string(),
        from: None,
        to: None,
        transport: None,
        status: None,
        message_id: None,
        conversation_id: None,
        reply_to: None,
        content: None,
        detail: None,
    }
}
