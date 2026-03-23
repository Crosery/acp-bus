use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "acp-bus", version, about = "Multi-agent collaboration CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Launch the TUI interface
    Tui {
        /// Working directory
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// Start JSON-RPC server over stdio
    Serve {
        /// Use stdio transport
        #[arg(long)]
        stdio: bool,
        /// Working directory
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// List saved channel snapshots
    Channels {
        /// Working directory to search
        #[arg(long, default_value = ".")]
        cwd: String,
    },
    /// Internal: run as MCP server for agent bus tools
    #[command(name = "mcp-server")]
    McpServer,
}

async fn mcp_call_socket(
    socket_path: &str,
    agent_name: &str,
    tool: &str,
    args: &serde_json::Value,
) -> String {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    if socket_path.is_empty() {
        return r#"{"error":"ACP_BUS_SOCKET not set"}"#.to_string();
    }

    let req = match tool {
        "bus_send_message" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            serde_json::json!({ "type": "send_message", "from": agent_name, "to": to, "content": content })
        }
        "bus_list_agents" => {
            serde_json::json!({ "type": "list_agents", "from": agent_name })
        }
        "bus_create_agent" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let adapter = args.get("adapter").and_then(|v| v.as_str()).unwrap_or("claude");
            let task = args.get("task").and_then(|v| v.as_str());
            let mut req = serde_json::json!({ "type": "create_agent", "from": agent_name, "name": name, "adapter": adapter });
            if let Some(task) = task { req["task"] = serde_json::json!(task); }
            req
        }
        "bus_remove_agent" => {
            let name = args.get("name").and_then(|v| v.as_str()).unwrap_or("");
            serde_json::json!({ "type": "remove_agent", "from": agent_name, "name": name })
        }
        "bus_send_and_wait" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let timeout = args.get("timeout_secs").and_then(|v| v.as_u64()).unwrap_or(120).min(300);
            serde_json::json!({ "type": "send_and_wait", "from": agent_name, "to": to, "content": content, "timeout_secs": timeout })
        }
        "bus_reply" => {
            let to = args.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content = args.get("content").and_then(|v| v.as_str()).unwrap_or("");
            let in_reply_to = args.get("in_reply_to").and_then(|v| v.as_u64());
            let mut req = serde_json::json!({ "type": "reply", "from": agent_name, "to": to, "content": content });
            if let Some(id) = in_reply_to { req["in_reply_to"] = serde_json::json!(id); }
            req
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Tui { cwd } => {
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();

            // Setup terminal
            crossterm::terminal::enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            crossterm::execute!(
                stdout,
                crossterm::terminal::EnterAlternateScreen,
                crossterm::event::EnableBracketedPaste,
                crossterm::event::EnableMouseCapture,
            )?;
            let backend = ratatui::backend::CrosstermBackend::new(stdout);
            let mut terminal = ratatui::Terminal::new(backend)?;

            let mut app = acp_tui::App::new(cwd);
            let result = app.run(&mut terminal).await;

            // Restore terminal
            crossterm::terminal::disable_raw_mode()?;
            crossterm::execute!(
                terminal.backend_mut(),
                crossterm::terminal::LeaveAlternateScreen,
                crossterm::event::DisableBracketedPaste,
                crossterm::event::DisableMouseCapture,
            )?;
            terminal.show_cursor()?;

            result?;
        }
        Commands::Serve { stdio, cwd } => {
            if !stdio {
                anyhow::bail!("only --stdio transport is supported");
            }
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();
            acp_server::serve_stdio(cwd).await?;
        }
        Commands::McpServer => {
            use std::io::Write;
            use serde_json::json;
            use tokio::io::{AsyncBufReadExt, BufReader};

            let socket_path = std::env::var("ACP_BUS_SOCKET").unwrap_or_default();
            let agent_name =
                std::env::var("ACP_BUS_AGENT_NAME").unwrap_or_else(|_| "unknown".into());

            let stdin = tokio::io::stdin();
            let mut lines = BufReader::new(stdin).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let msg: serde_json::Value = match serde_json::from_str(&line) {
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
                                "inputSchema": { "type": "object", "properties": {} }
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
                                "description": "Send a message to another agent and wait for their reply (synchronous). Blocks until the target calls bus_reply or timeout.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "to": { "type": "string", "description": "Target agent name" },
                                        "content": { "type": "string", "description": "Message content" },
                                        "timeout_secs": { "type": "integer", "description": "Timeout in seconds (default 120, max 300)" }
                                    },
                                    "required": ["to", "content"]
                                }
                            },
                            {
                                "name": "bus_reply",
                                "description": "Reply to a message from another agent. Use after receiving a send_and_wait message.",
                                "inputSchema": {
                                    "type": "object",
                                    "properties": {
                                        "to": { "type": "string", "description": "Agent to reply to" },
                                        "content": { "type": "string", "description": "Reply content" },
                                        "in_reply_to": { "type": "integer", "description": "Original message ID (optional)" }
                                    },
                                    "required": ["to", "content"]
                                }
                            }
                        ]
                    }),
                    "tools/call" => {
                        let params = msg.get("params").cloned().unwrap_or(json!({}));
                        let tool_name =
                            params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = params.get("arguments").cloned().unwrap_or(json!({}));

                        let resp =
                            mcp_call_socket(&socket_path, &agent_name, tool_name, &args).await;
                        json!({ "content": [{ "type": "text", "text": resp }] })
                    }
                    _ => {
                        let resp = json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32601, "message": format!("method not found: {method}") }
                        });
                        let mut stdout = std::io::stdout().lock();
                        let _ = serde_json::to_writer(&mut stdout, &resp);
                        let _ = stdout.write_all(b"\n");
                        let _ = stdout.flush();
                        continue;
                    }
                };

                let resp = json!({ "jsonrpc": "2.0", "id": id, "result": result });
                let mut stdout = std::io::stdout().lock();
                let _ = serde_json::to_writer(&mut stdout, &resp);
                let _ = stdout.write_all(b"\n");
                let _ = stdout.flush();
            }
        }
        Commands::Channels { cwd } => {
            let cwd = std::fs::canonicalize(&cwd)?.to_string_lossy().to_string();
            let snapshots = acp_core::store::list_snapshots(&cwd).await?;
            if snapshots.is_empty() {
                println!("No saved channels found.");
            } else {
                for s in &snapshots {
                    println!(
                        "{} | {} | {} msgs | {}",
                        s.channel_id, s.saved_at, s.msg_count, s.agents
                    );
                }
            }
        }
    }

    Ok(())
}
