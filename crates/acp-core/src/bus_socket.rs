use std::path::PathBuf;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::client::BusEvent;

pub async fn start_bus_socket(
    channel_id: &str,
    bus_tx: mpsc::UnboundedSender<BusEvent>,
) -> anyhow::Result<PathBuf> {
    let path = PathBuf::from(format!("/tmp/acp-bus-{channel_id}.sock"));

    // Clean up stale socket
    let _ = std::fs::remove_file(&path);

    let listener = UnixListener::bind(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    debug!(path = %path.display(), "bus socket listening");

    let cleanup_path = path.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(e) => {
                    warn!("bus socket accept error: {e}");
                    break;
                }
            };

            let bus_tx = bus_tx.clone();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    let resp = handle_line(&line, &bus_tx).await;
                    let mut out = resp;
                    out.push('\n');
                    if writer.write_all(out.as_bytes()).await.is_err() {
                        break;
                    }
                }
            });
        }
        let _ = std::fs::remove_file(&cleanup_path);
    });

    Ok(path)
}

async fn handle_line(line: &str, bus_tx: &mpsc::UnboundedSender<BusEvent>) -> String {
    let msg: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => return r#"{"error":"invalid json"}"#.to_string(),
    };

    let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match msg_type {
        "send_message" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let to = msg.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let _ = bus_tx.send(BusEvent::SendMessage {
                from_agent: from.to_string(),
                to_agent: to.to_string(),
                content: content.to_string(),
                reply_tx,
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                Ok(Ok(result)) => serde_json::json!({
                    "ok": result.delivered,
                    "messageId": result.message_id,
                    "delivered": result.delivered,
                    "error": result.error,
                })
                .to_string(),
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        "list_agents" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg
                .get("from")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let _ = bus_tx.send(BusEvent::ListAgents {
                from_agent: from.to_string(),
                reply_tx,
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                Ok(Ok(agents)) => {
                    let list: Vec<serde_json::Value> = agents
                        .iter()
                        .map(|a| serde_json::json!({"name": a.name, "status": a.status, "adapter": a.adapter}))
                        .collect();
                    serde_json::json!({"agents": list}).to_string()
                }
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        "create_agent" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
            let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let adapter = msg.get("adapter").and_then(|v| v.as_str()).unwrap_or("claude");
            let task = msg.get("task").and_then(|v| v.as_str()).map(|s| s.to_string());
            let _ = bus_tx.send(BusEvent::CreateAgent {
                from_agent: from.to_string(),
                name: name.to_string(),
                adapter: adapter.to_string(),
                task,
                reply_tx,
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                Ok(Ok(result)) => serde_json::json!({
                    "ok": result.ok,
                    "error": result.error,
                }).to_string(),
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        "remove_agent" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
            let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let _ = bus_tx.send(BusEvent::RemoveAgent {
                from_agent: from.to_string(),
                name: name.to_string(),
                reply_tx,
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                Ok(Ok(result)) => serde_json::json!({
                    "ok": result.ok,
                    "error": result.error,
                }).to_string(),
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        "send_and_wait" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
            let to = msg.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let timeout_secs = msg.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(120);
            let _ = bus_tx.send(BusEvent::SendAndWait {
                from_agent: from.to_string(),
                to_agent: to.to_string(),
                content: content.to_string(),
                timeout_secs,
                reply_tx,
            });
            // send_and_wait may take a long time — use the requested timeout + buffer
            let timeout = std::time::Duration::from_secs(timeout_secs + 10);
            match tokio::time::timeout(timeout, reply_rx).await {
                Ok(Ok(result)) => serde_json::json!({
                    "ok": result.ok,
                    "reply": result.reply_content,
                    "from": result.from_agent,
                    "error": result.error,
                }).to_string(),
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        "reply" => {
            let (reply_tx, reply_rx) = oneshot::channel();
            let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
            let to = msg.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let in_reply_to = msg.get("in_reply_to").and_then(|v| v.as_u64());
            let _ = bus_tx.send(BusEvent::Reply {
                from_agent: from.to_string(),
                to_agent: to.to_string(),
                content: content.to_string(),
                in_reply_to,
                reply_tx,
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                Ok(Ok(result)) => serde_json::json!({
                    "ok": result.delivered,
                    "messageId": result.message_id,
                    "error": result.error,
                }).to_string(),
                _ => r#"{"error":"timeout"}"#.to_string(),
            }
        }
        _ => r#"{"error":"unknown type"}"#.to_string(),
    }
}
