use std::io::Write;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

#[tokio::main]
async fn main() {
    let socket_path = std::env::var("ACP_BUS_SOCKET").unwrap_or_default();
    let agent_name = std::env::var("ACP_BUS_AGENT_NAME").unwrap_or_else(|_| "unknown".into());

    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("");

        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "acp-bus-mcp", "version": "0.1.0" }
            }),
            "notifications/initialized" => continue,
            "tools/list" => json!({
                "tools": [
                    {
                        "name": "bus_send_message",
                        "description": "Send a message to another agent in the acp-bus channel",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "to": { "type": "string", "description": "Target agent name" },
                                "content": { "type": "string", "description": "Message content" }
                            },
                            "required": ["to", "content"]
                        }
                    },
                    {
                        "name": "bus_list_agents",
                        "description": "List all agents in the acp-bus channel with their status",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    },
                    {
                        "name": "bus_create_agent",
                        "description": "Create a new agent and optionally assign a task. Only callable by the main agent.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Agent name" },
                                "adapter": { "type": "string", "description": "Adapter: claude, c1, c2, gemini, codex" },
                                "task": { "type": "string", "description": "Optional task to assign after agent connects" }
                            },
                            "required": ["name", "adapter"]
                        }
                    },
                    {
                        "name": "bus_remove_agent",
                        "description": "Remove an agent. Only callable by the main agent.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Agent name to remove" }
                            },
                            "required": ["name"]
                        }
                    },
                    {
                        "name": "bus_send_and_wait",
                        "description": "Send a message to another agent and wait for their reply (synchronous). The call blocks until the target agent calls bus_reply, or timeout.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "to": { "type": "string", "description": "Target agent name" },
                                "content": { "type": "string", "description": "Message content" },
                                "timeout_secs": { "type": "integer", "description": "Timeout in seconds (default 300, max 600)" }
                            },
                            "required": ["to", "content"]
                        }
                    },
                    {
                        "name": "bus_reply",
                        "description": "Reply to a message from another agent. Use this after receiving a message via bus_send_and_wait to send your response back.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "to": { "type": "string", "description": "Agent to reply to" },
                                "content": { "type": "string", "description": "Reply content" },
                                "in_reply_to": { "type": "integer", "description": "Original message ID (optional)" }
                            },
                            "required": ["to", "content"]
                        }
                    },
                    {
                        "name": "bus_create_group",
                        "description": "Create a discussion group and invite members. Group messages are broadcast to all members.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "name": { "type": "string", "description": "Group name" },
                                "members": {
                                    "type": "array",
                                    "items": { "type": "string" },
                                    "description": "Initial member names"
                                }
                            },
                            "required": ["name", "members"]
                        }
                    },
                    {
                        "name": "bus_group_add",
                        "description": "Add a member to an existing group.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "group": { "type": "string", "description": "Group name" },
                                "member": { "type": "string", "description": "Agent name to add" }
                            },
                            "required": ["group", "member"]
                        }
                    },
                    {
                        "name": "bus_group_message",
                        "description": "Send a message to all members of a group. IMPORTANT: set 'rounds' to control how many discussion rounds happen. Default is 1 (single round). For debates or multi-turn discussions, set rounds=3 or higher.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "group": { "type": "string", "description": "Group name" },
                                "content": { "type": "string", "description": "Message content" },
                                "rounds": { "type": "integer", "description": "Number of discussion rounds (1-10, default 3). Each round prompts ALL members sequentially.", "default": 3 }
                            },
                            "required": ["group", "content"]
                        }
                    }
                ]
            }),
            "tools/call" => {
                let params = msg.get("params").cloned().unwrap_or(json!({}));
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));

                let resp = call_socket(&socket_path, &agent_name, tool_name, &args).await;
                json!({ "content": [{ "type": "text", "text": resp }] })
            }
            _ => {
                // Unknown method — send error
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32601, "message": format!("method not found: {method}") }
                });
                write_response(&resp);
                continue;
            }
        };

        let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
        write_response(&resp);
    }
}

fn write_response(resp: &Value) {
    let mut stdout = std::io::stdout().lock();
    let _ = serde_json::to_writer(&mut stdout, resp);
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}

async fn call_socket(socket_path: &str, agent_name: &str, tool: &str, args: &Value) -> String {
    if socket_path.is_empty() {
        return r#"{"error":"ACP_BUS_SOCKET not set"}"#.to_string();
    }

    let req = match tool {
        "bus_send_message" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            json!({ "type": "send_message", "from": agent_name, "to": to, "content": content })
        }
        "bus_list_agents" => {
            json!({ "type": "list_agents", "from": agent_name })
        }
        "bus_create_agent" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let adapter = args
                .get("adapter")
                .and_then(|v| v.as_str())
                .unwrap_or("claude");
            let task = args.get("task").and_then(|v| v.as_str());
            let mut req = json!({
                "type": "create_agent",
                "from": agent_name,
                "name": name,
                "adapter": adapter
            });
            if let Some(task) = task {
                req["task"] = json!(task);
            }
            req
        }
        "bus_remove_agent" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            json!({
                "type": "remove_agent",
                "from": agent_name,
                "name": name
            })
        }
        "bus_send_and_wait" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let timeout = args
                .get("timeout_secs")
                .and_then(|v| v.as_u64())
                .unwrap_or(300)
                .min(600);
            json!({
                "type": "send_and_wait",
                "from": agent_name,
                "to": to,
                "content": content,
                "timeout_secs": timeout
            })
        }
        "bus_reply" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let in_reply_to = args.get("in_reply_to").and_then(|v| v.as_u64());
            let mut req = json!({
                "type": "reply",
                "from": agent_name,
                "to": to,
                "content": content
            });
            if let Some(id) = in_reply_to {
                req["in_reply_to"] = json!(id);
            }
            req
        }
        "bus_create_group" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let members: Vec<String> = args
                .get("members")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            json!({
                "type": "create_group",
                "from": agent_name,
                "name": name,
                "members": members
            })
        }
        "bus_group_add" => {
            let group = args.get("group").and_then(|v| v.as_str()).unwrap_or("");
            let member = args.get("member").and_then(|v| v.as_str()).unwrap_or("");
            json!({
                "type": "group_add",
                "from": agent_name,
                "group": group,
                "member": member
            })
        }
        "bus_group_message" => {
            let group = args.get("group").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let rounds = args.get("rounds").and_then(|v| v.as_u64()).unwrap_or(3);
            json!({
                "type": "group_message",
                "from": agent_name,
                "group": group,
                "content": content,
                "rounds": rounds
            })
        }
        _ => return r#"{"error":"unknown tool"}"#.to_string(),
    };

    let stream = match UnixStream::connect(socket_path).await {
        Ok(s) => s,
        Err(e) => return format!(r#"{{"error":"connect failed: {e}"}}"#),
    };

    let (reader, mut writer) = stream.into_split();
    let mut line = req.to_string();
    line.push('\n');
    if writer.write_all(line.as_bytes()).await.is_err() {
        return r#"{"error":"write failed"}"#.to_string();
    }
    let _ = writer.shutdown().await;

    let mut lines = BufReader::new(reader).lines();
    match lines.next_line().await {
        Ok(Some(resp)) => resp,
        Ok(None) => r#"{"error":"no response"}"#.to_string(),
        Err(e) => format!(r#"{{"error":"read failed: {e}"}}"#),
    }
}
