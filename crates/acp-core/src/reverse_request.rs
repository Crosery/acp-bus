//! Handles reverse JSON-RPC requests from agents back to the host.
//!
//! Agents can request filesystem access (fs/read_text_file, fs/write_text_file),
//! terminal management (terminal/*), bus operations (bus/*), and permission
//! grants (session/request_permission).

use std::path::PathBuf;
use std::sync::Arc;

use acp_protocol::{encode_error, encode_response, RpcMessage};
use tokio::sync::{mpsc, oneshot};
use tracing::warn;

use crate::bus_types::BusEvent;
use crate::terminal::TerminalManager;

/// Handle a reverse request from the agent and write the response to stdin.
pub(crate) async fn handle_reverse_request(
    msg: RpcMessage,
    stdin_tx: &mpsc::Sender<String>,
    terminal_mgr: &Arc<TerminalManager>,
    adapter_name: &str,
    bus_tx: &Option<mpsc::UnboundedSender<BusEvent>>,
    agent_name: &str,
    cwd: &str,
) {
    let method = msg.method.as_deref().unwrap_or("");
    let id = msg.id.as_ref().unwrap();
    let params = msg.params.unwrap_or(serde_json::json!({}));

    let response = match method {
        "session/request_permission" => {
            handle_permission(id, &params)
        }
        "fs/read_text_file" => {
            handle_fs_read(id, &params, cwd).await
        }
        "fs/write_text_file" => {
            handle_fs_write(id, &params, cwd).await
        }
        "terminal/create" => {
            match terminal_mgr.handle_create(&params).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/output" => {
            let tid = params.get("terminalId").and_then(|v| v.as_str()).unwrap_or("");
            match terminal_mgr.handle_output(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/wait_for_exit" => {
            let tid = params.get("terminalId").and_then(|v| v.as_str()).unwrap_or("");
            match terminal_mgr.handle_wait(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/kill" => {
            let tid = params.get("terminalId").and_then(|v| v.as_str()).unwrap_or("");
            match terminal_mgr.handle_kill(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/release" => {
            let tid = params.get("terminalId").and_then(|v| v.as_str()).unwrap_or("");
            match terminal_mgr.handle_release(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "bus/send_message" => {
            handle_bus_send(id, &params, bus_tx, agent_name).await
        }
        "bus/list_agents" => {
            handle_bus_list(id, bus_tx, agent_name).await
        }
        _ => {
            warn!(adapter = adapter_name, method, "unknown reverse request");
            encode_error(id, -32601, &format!("method not found: {method}"))
        }
    };

    let _ = stdin_tx.send(response).await;
}

// ── Permission ──────────────────────────────────────────────────────

fn handle_permission(id: &serde_json::Value, params: &serde_json::Value) -> String {
    // Auto-allow (yolo mode)
    let options = params.get("options").and_then(|v| v.as_array());
    let allow_id = options
        .and_then(|opts| {
            opts.iter().find_map(|opt| {
                let kind = opt.get("kind").and_then(|v| v.as_str())?;
                if kind == "allow_once" || kind == "allow_always" {
                    opt.get("optionId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "allow".to_string());

    encode_response(
        id,
        serde_json::json!({
            "outcome": { "outcome": "selected", "optionId": allow_id }
        }),
    )
}

// ── Filesystem ──────────────────────────────────────────────────────

async fn handle_fs_read(
    id: &serde_json::Value,
    params: &serde_json::Value,
    cwd: &str,
) -> String {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    if path.is_empty() {
        return encode_error(id, -32602, "missing path");
    }
    match validate_path_within_cwd(path, cwd) {
        Err(e) => encode_error(id, -32600, &format!("path outside working directory: {e}")),
        Ok(resolved) => match tokio::fs::read_to_string(&resolved).await {
            Ok(content) => {
                let line = params.get("line").and_then(|v| v.as_u64());
                let limit = params.get("limit").and_then(|v| v.as_u64());
                let result = if line.is_some() || limit.is_some() {
                    let lines: Vec<&str> = content.lines().collect();
                    let total = lines.len();
                    let start = line.unwrap_or(1).max(1) as usize - 1;
                    if start >= total {
                        serde_json::json!({ "content": "" })
                    } else {
                        let end = if let Some(l) = limit {
                            (start + l as usize).min(total)
                        } else {
                            total
                        };
                        let slice = &lines[start..end];
                        serde_json::json!({ "content": slice.join("\n") })
                    }
                } else {
                    serde_json::json!({ "content": content })
                };
                encode_response(id, result)
            }
            Err(_) => encode_response(id, serde_json::json!({ "content": "" })),
        },
    }
}

async fn handle_fs_write(
    id: &serde_json::Value,
    params: &serde_json::Value,
    cwd: &str,
) -> String {
    let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
    let content = params.get("content").and_then(|v| v.as_str());
    match (path.is_empty(), content) {
        (true, _) | (_, None) => encode_error(id, -32602, "missing path or content"),
        (false, Some(content)) => match validate_path_within_cwd(path, cwd) {
            Err(e) => {
                encode_error(id, -32600, &format!("path outside working directory: {e}"))
            }
            Ok(resolved) => {
                if let Some(parent) = resolved.parent() {
                    let _ = tokio::fs::create_dir_all(parent).await;
                }
                match tokio::fs::write(&resolved, content).await {
                    Ok(()) => encode_response(id, serde_json::json!({})),
                    Err(e) => encode_error(
                        id,
                        -32000,
                        &format!("cannot write: {}: {e}", resolved.display()),
                    ),
                }
            }
        },
    }
}

// ── Bus operations ──────────────────────────────────────────────────

async fn handle_bus_send(
    id: &serde_json::Value,
    params: &serde_json::Value,
    bus_tx: &Option<mpsc::UnboundedSender<BusEvent>>,
    agent_name: &str,
) -> String {
    let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("");
    let content_text = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
    let Some(tx) = bus_tx else {
        return encode_error(id, -32000, "bus not available");
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = tx.send(BusEvent::SendMessage {
        from_agent: agent_name.to_string(),
        to_agent: to.to_string(),
        content: content_text.to_string(),
        reply_tx,
    });
    match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
        Ok(Ok(result)) => encode_response(
            id,
            serde_json::json!({
                "ok": result.delivered,
                "messageId": result.message_id,
                "delivered": result.delivered,
                "error": result.error,
            }),
        ),
        _ => encode_error(id, -32000, "send_message timeout"),
    }
}

async fn handle_bus_list(
    id: &serde_json::Value,
    bus_tx: &Option<mpsc::UnboundedSender<BusEvent>>,
    agent_name: &str,
) -> String {
    let Some(tx) = bus_tx else {
        return encode_error(id, -32000, "bus not available");
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    let _ = tx.send(BusEvent::ListAgents {
        from_agent: agent_name.to_string(),
        reply_tx,
    });
    match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
        Ok(Ok(agents)) => {
            let list: Vec<serde_json::Value> = agents
                .iter()
                .map(|a| {
                    serde_json::json!({
                        "name": a.name,
                        "status": a.status,
                        "adapter": a.adapter,
                        "activity": a.activity,
                        "activeSecs": a.active_secs,
                        "currentTask": a.current_task,
                        "inboxDepth": a.inbox_depth,
                        "waitingFor": a.waiting_for,
                    })
                })
                .collect();
            encode_response(id, serde_json::json!({"agents": list}))
        }
        _ => encode_error(id, -32000, "list_agents timeout"),
    }
}

// ── Path validation ─────────────────────────────────────────────────

/// Validate that a path is within the allowed working directory.
/// Returns the resolved absolute path on success.
fn validate_path_within_cwd(path: &str, cwd: &str) -> Result<PathBuf, String> {
    let cwd_canonical = match std::fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(_) => return Err("cannot resolve working directory".to_string()),
    };

    let target = std::path::Path::new(path);
    let abs_target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        cwd_canonical.join(target)
    };

    let resolved = if abs_target.exists() {
        std::fs::canonicalize(&abs_target)
    } else if let Some(parent) = abs_target.parent() {
        if parent.exists() {
            std::fs::canonicalize(parent)
                .map(|p| p.join(abs_target.file_name().unwrap_or_default()))
        } else {
            return Err("parent directory does not exist".to_string());
        }
    } else {
        return Err("invalid path".to_string());
    };

    match resolved {
        Ok(resolved) => {
            if resolved.starts_with(&cwd_canonical) {
                Ok(resolved)
            } else {
                Err("path outside working directory".to_string())
            }
        }
        Err(_) => Err("cannot resolve path".to_string()),
    }
}
