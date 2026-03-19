use acp_core::channel::Channel;
use acp_core::registry::Registry;
use acp_protocol::{decode, encode_error, encode_response, error_codes};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::Mutex;
use tracing::{error, info};

pub async fn serve_stdio(cwd: String) -> anyhow::Result<()> {
    let registry = Arc::new(Mutex::new(Registry::new()));

    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin);
    let mut line = String::new();

    info!("acp-bus server started (stdio)");

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF
            Err(e) => {
                error!("stdin read error: {e}");
                break;
            }
            Ok(_) => {}
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg = match decode(trimmed) {
            Some(msg) => msg,
            None => continue,
        };

        if !msg.is_request() {
            continue;
        }

        let id = msg.id.as_ref().unwrap().clone();
        let method = msg.method.as_deref().unwrap_or("");
        let params = msg.params.clone().unwrap_or(serde_json::json!({}));

        let response = match method {
            "channel/create" => {
                let adapter = params
                    .get("adapter")
                    .and_then(|v| v.as_str())
                    .unwrap_or("claude");
                let ch = Channel::new(cwd.clone());
                let channel_id = ch.channel_id.clone();
                let mut reg = registry.lock().await;
                reg.add(ch);
                encode_response(
                    &id,
                    serde_json::json!({
                        "channel_id": channel_id,
                        "adapter": adapter,
                    }),
                )
            }
            "channel/list" => {
                let reg = registry.lock().await;
                let channels = reg.list();
                let list: Vec<serde_json::Value> = channels
                    .iter()
                    .map(|c| serde_json::json!({ "id": c.id, "is_active": c.is_active }))
                    .collect();
                encode_response(&id, serde_json::json!({ "channels": list }))
            }
            "channel/close" => {
                let channel_id = params.get("channel_id").and_then(|v| v.as_str());
                let mut reg = registry.lock().await;
                let cid = channel_id
                    .map(|s| s.to_string())
                    .or_else(|| reg.active_id().map(|s| s.to_string()));
                if let Some(cid) = cid {
                    if let Some(ch) = reg.get(&cid) {
                        ch.lock().await.close();
                    }
                    reg.remove(&cid);
                }
                encode_response(&id, serde_json::json!({}))
            }
            "channel/post" => {
                let text = params.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let from = params.get("from").and_then(|v| v.as_str()).unwrap_or("you");
                let reg = registry.lock().await;
                if let Some(ch) = reg.active() {
                    ch.lock().await.post(from, text, false);
                }
                encode_response(&id, serde_json::json!({}))
            }
            "channel/read" => {
                let last_n = params.get("last_n").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let reg = registry.lock().await;
                if let Some(ch) = reg.active() {
                    let ch = ch.lock().await;
                    let msgs = ch.read(last_n);
                    let messages: Vec<serde_json::Value> = msgs
                        .iter()
                        .map(|m| {
                            serde_json::json!({
                                "from": m.from,
                                "content": m.content,
                                "timestamp": m.timestamp,
                            })
                        })
                        .collect();
                    encode_response(&id, serde_json::json!({ "messages": messages }))
                } else {
                    encode_response(&id, serde_json::json!({ "messages": [] }))
                }
            }
            "agent/list" => {
                let reg = registry.lock().await;
                if let Some(ch) = reg.active() {
                    let ch = ch.lock().await;
                    let agents = ch.list_agents();
                    encode_response(&id, serde_json::json!({ "agents": agents }))
                } else {
                    encode_response(&id, serde_json::json!({ "agents": [] }))
                }
            }
            "adapter/list" => {
                let adapters = acp_core::adapter::list_detailed();
                let list: Vec<serde_json::Value> = adapters
                    .iter()
                    .map(|(name, desc)| serde_json::json!({ "name": name, "description": desc }))
                    .collect();
                encode_response(&id, serde_json::json!({ "adapters": list }))
            }
            _ => encode_error(
                &id,
                error_codes::METHOD_NOT_FOUND,
                &format!("method not found: {method}"),
            ),
        };

        stdout.write_all(response.as_bytes()).await?;
        stdout.write_all(b"\n").await?;
        stdout.flush().await?;
    }

    info!("acp-bus server stopped");
    Ok(())
}
