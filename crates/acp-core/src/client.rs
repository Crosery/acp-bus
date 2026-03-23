use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use acp_protocol::handshake::{AuthenticateParams, InitializeParams, SessionNewParams};
use acp_protocol::session::{PromptContent, SessionCancelParams, SessionPromptParams};
use acp_protocol::{
    decode, encode_error, encode_request, encode_response, next_id, LineBuffer, RpcMessage,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot, Mutex};
use tracing::{debug, info, warn};

use crate::adapter::AdapterConfig;
use crate::terminal::TerminalManager;

/// Events emitted by the ACP client
#[derive(Debug, Clone)]
pub enum ClientEvent {
    SessionUpdate(serde_json::Value),
    Exited { code: Option<i32> },
}

#[derive(Debug)]
pub enum BusEvent {
    SendMessage {
        from_agent: String,
        to_agent: String,
        content: String,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    ListAgents {
        from_agent: String,
        reply_tx: oneshot::Sender<Vec<AgentInfo>>,
    },
}

#[derive(Debug, Clone)]
pub struct BusSendResult {
    pub message_id: Option<u64>,
    pub delivered: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentInfo {
    pub name: String,
    pub status: String,
    pub adapter: String,
    pub activity: Option<String>,
    pub active_secs: Option<i64>,
}

/// Shared activity tracker — updated on every session/update event
pub type LastActivity = Arc<Mutex<std::time::Instant>>;

/// Pending request callback
type PendingCallback = oneshot::Sender<Result<serde_json::Value, acp_protocol::RpcError>>;

/// ACP Client: manages a spawned agent process with full ACP handshake
pub struct AcpClient {
    adapter: AdapterConfig,
    stdin_tx: Option<mpsc::Sender<String>>,
    pending: Arc<Mutex<HashMap<u64, PendingCallback>>>,
    pub session_id: Option<String>,
    pub alive: bool,
    event_tx: mpsc::UnboundedSender<ClientEvent>,
    terminal_mgr: Arc<TerminalManager>,
    child: Option<Child>,
    pub last_activity: LastActivity,
    /// PID of the spawned child process (for force-kill)
    pub child_pid: u32,
}

impl AcpClient {
    /// Spawn the agent process, perform ACP handshake (initialize → authenticate? → session/new).
    /// Returns the client and an event receiver for session/update notifications.
    pub async fn start(
        adapter: AdapterConfig,
        cwd: String,
        bus_tx: Option<mpsc::UnboundedSender<BusEvent>>,
        agent_name: String,
    ) -> anyhow::Result<(Self, mpsc::UnboundedReceiver<ClientEvent>)> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let mut env_vars: HashMap<String, String> = std::env::vars().collect();
        for (k, v) in &adapter.env {
            env_vars.insert(k.clone(), v.clone());
        }

        info!(
            adapter = %adapter.name,
            cmd = %adapter.cmd,
            cwd = %cwd,
            "spawning agent"
        );

        let mut cmd = Command::new(&adapter.cmd);
        cmd.args(&adapter.args)
            .envs(&env_vars)
            .current_dir(&cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // Put child in its own process group so we can kill the whole group
        #[cfg(unix)]
        {
            #[allow(unused_imports)]
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }

        let mut child = cmd.spawn()?;

        let pid = child.id().unwrap_or(0);
        info!(adapter = %adapter.name, pid, "agent spawned");

        let child_stdout = child.stdout.take().unwrap();
        let child_stderr = child.stderr.take().unwrap();
        let child_stdin = child.stdin.take().unwrap();

        // Stdin writer task
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(256);
        let stdin_writer = tokio::spawn(async move {
            let mut stdin = child_stdin;
            while let Some(line) = stdin_rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if stdin.write_all(b"\n").await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });

        // Stderr logger task
        tokio::spawn(async move {
            let mut reader = BufReader::new(child_stderr);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            let truncated: String = trimmed.chars().take(200).collect();
                            debug!(target: "acp_stderr", "{}", truncated);
                        }
                    }
                }
            }
        });

        let pending: Arc<Mutex<HashMap<u64, PendingCallback>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let terminal_mgr = Arc::new(TerminalManager::new());
        let last_activity: LastActivity = Arc::new(Mutex::new(std::time::Instant::now()));

        // Stdout reader + dispatch task
        let pending_clone = pending.clone();
        let event_tx_clone = event_tx.clone();
        let stdin_tx_clone = stdin_tx.clone();
        let terminal_mgr_clone = terminal_mgr.clone();
        let last_activity_clone = last_activity.clone();
        let adapter_name = adapter.name.clone();
        let bus_tx_clone = bus_tx.clone();
        let agent_name_clone = agent_name.clone();
        let cwd_clone = cwd.clone();

        tokio::spawn(async move {
            let mut reader = BufReader::new(child_stdout);
            let mut line_buf = LineBuffer::new();
            let mut raw = vec![0u8; 8192];
            loop {
                match reader.read(&mut raw).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&raw[..n]);
                        let lines = line_buf.feed(&chunk);
                        for line in lines {
                            if let Some(msg) = decode(&line) {
                                // Touch activity on any message from agent
                                *last_activity_clone.lock().await = std::time::Instant::now();
                                dispatch_message(
                                    msg,
                                    &pending_clone,
                                    &event_tx_clone,
                                    &stdin_tx_clone,
                                    &terminal_mgr_clone,
                                    &adapter_name,
                                    &bus_tx_clone,
                                    &agent_name_clone,
                                    &cwd_clone,
                                )
                                .await;
                            }
                        }
                    }
                }
            }
            // Agent exited — notify
            let _ = event_tx_clone.send(ClientEvent::Exited { code: None });
        });

        let mut client = Self {
            adapter,
            stdin_tx: Some(stdin_tx),
            pending,
            session_id: None,
            alive: false,
            event_tx,
            terminal_mgr,
            child: Some(child),
            last_activity,
            child_pid: pid,
        };

        // === ACP Handshake ===

        // 1. initialize (with retries)
        let init_params = InitializeParams::default_with_terminal(client.adapter.terminal);
        let init_result = client
            .request_with_retry("initialize", serde_json::to_value(&init_params)?, 20_000, 2)
            .await?;

        let proto_version = init_result.get("protocolVersion").and_then(|v| v.as_u64());
        info!(
            adapter = %client.adapter.name,
            proto = ?proto_version,
            "initialize ok"
        );

        // 2. authenticate (if needed)
        if let Some(auth_method) = &client.adapter.auth_method {
            let auth_params = AuthenticateParams {
                method_id: auth_method.clone(),
                meta: client
                    .adapter
                    .auth_api_key
                    .as_ref()
                    .map(|key| serde_json::json!({ "api-key": key })),
            };
            client
                .request_with_retry(
                    "authenticate",
                    serde_json::to_value(&auth_params)?,
                    15_000,
                    0,
                )
                .await?;
            info!(adapter = %client.adapter.name, method = %auth_method, "authenticate ok");
        }

        // 3. session/new (with retries)
        // Build _meta for ACP: system prompt append + disallowed tools
        let meta = {
            let mut meta = serde_json::Map::new();

            // Append system prompt at the real system level
            if let Some(ref sp) = client.adapter.system_prompt {
                meta.insert(
                    "systemPrompt".into(),
                    serde_json::json!({
                        "append": sp
                    }),
                );
            }

            // Disallow tools via claudeCode.options
            if !client.adapter.disallowed_tools.is_empty() {
                meta.insert(
                    "claudeCode".into(),
                    serde_json::json!({
                        "options": {
                            "disallowedTools": client.adapter.disallowed_tools
                        }
                    }),
                );
            }

            if meta.is_empty() {
                None
            } else {
                Some(serde_json::Value::Object(meta))
            }
        };

        let mcp_servers = if let Some(ref sp) = client.adapter.socket_path {
            let mcp_cmd = client.adapter.mcp_command.as_deref().unwrap_or("acp-bus-mcp");
            let mcp_args = if mcp_cmd.ends_with("acp-bus") || mcp_cmd.ends_with("acp-bus.exe") {
                serde_json::json!(["mcp-server"])
            } else {
                serde_json::json!([])
            };
            serde_json::json!([{
                "name": "acp-bus",
                "command": mcp_cmd,
                "args": mcp_args,
                "env": [
                    { "name": "ACP_BUS_SOCKET", "value": sp },
                    { "name": "ACP_BUS_AGENT_NAME", "value": &agent_name }
                ]
            }])
        } else {
            serde_json::json!([])
        };

        let session_params = SessionNewParams {
            cwd: cwd.clone(),
            mcp_servers,
            meta,
        };
        let session_result = client
            .request_with_retry(
                "session/new",
                serde_json::to_value(&session_params)?,
                60_000,
                2,
            )
            .await?;

        client.session_id = session_result
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        client.alive = true;

        info!(
            adapter = %client.adapter.name,
            session = ?client.session_id,
            "handshake complete"
        );

        // Watch for child exit
        let event_tx2 = client.event_tx.clone();
        let pending2 = client.pending.clone();
        let _stdin_writer = stdin_writer; // keep alive
        if let Some(mut child) = client.child.take() {
            tokio::spawn(async move {
                let status = child.wait().await;
                let code = status.ok().and_then(|s| s.code());
                // Clear all pending requests
                let mut pending = pending2.lock().await;
                for (_, cb) in pending.drain() {
                    let _ = cb.send(Err(acp_protocol::RpcError {
                        code: -32000,
                        message: format!("client exited (code={code:?})"),
                        data: None,
                    }));
                }
                let _ = event_tx2.send(ClientEvent::Exited { code });
            });
        }

        Ok((client, event_rx))
    }

    /// Send a prompt to the agent (async, returns stop_reason).
    pub async fn prompt(&self, text: &str) -> anyhow::Result<String> {
        let session_id = self
            .session_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no session"))?;

        let params = SessionPromptParams {
            session_id: session_id.clone(),
            prompt: vec![PromptContent::Text {
                text: text.to_string(),
            }],
        };

        let result = self
            .request("session/prompt", serde_json::to_value(&params)?)
            .await?;

        let stop_reason = result
            .get("stopReason")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(stop_reason)
    }

    /// Cancel the current prompt.
    pub async fn cancel(&self) {
        if let Some(session_id) = &self.session_id {
            let params = SessionCancelParams {
                session_id: session_id.clone(),
            };
            if let Ok(val) = serde_json::to_value(&params) {
                self.send_notification("session/cancel", val).await;
            }
        }
    }

    /// Stop the agent process gracefully.
    pub async fn stop(&mut self) {
        self.alive = false;
        self.stdin_tx = None; // drop sender → stdin writer exits → child gets EOF
        self.terminal_mgr.cleanup().await;
    }

    /// Force-kill the agent process immediately (SIGKILL).
    pub fn force_kill(&self) {
        if self.child_pid != 0 {
            #[cfg(unix)]
            unsafe {
                // Kill process group to clean up all children
                libc::kill(-(self.child_pid as i32), libc::SIGKILL);
            }
        }
    }

    /// Send a JSON-RPC request, wait for response.
    /// Uses activity-based timeout: only times out if agent has been
    /// completely silent (no events at all) for the idle period.
    async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let id = next_id();
        let (tx, rx) = oneshot::channel();

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id, tx);
        }

        // Touch activity when we send the request
        *self.last_activity.lock().await = std::time::Instant::now();

        let msg = encode_request(id, method, params);
        if let Some(stdin_tx) = &self.stdin_tx {
            stdin_tx.send(msg).await?;
        } else {
            anyhow::bail!("stdin closed");
        }

        // Activity-based timeout: check every 30s if agent is still active.
        // Only timeout if no activity for 15 minutes.
        const IDLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(900);
        const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

        let activity = self.last_activity.clone();
        let pending_ref = self.pending.clone();
        let method_name = method.to_string();
        let req_id = id;

        tokio::pin!(rx);
        loop {
            tokio::select! {
                result = &mut rx => {
                    return match result {
                        Ok(Ok(result)) => Ok(result),
                        Ok(Err(rpc_err)) => Err(anyhow::anyhow!("RPC error: {rpc_err}")),
                        Err(_) => Err(anyhow::anyhow!("request cancelled")),
                    };
                }
                _ = tokio::time::sleep(CHECK_INTERVAL) => {
                    let last = *activity.lock().await;
                    if last.elapsed() > IDLE_TIMEOUT {
                        pending_ref.lock().await.remove(&req_id);
                        return Err(anyhow::anyhow!(
                            "{method_name} 超时：agent 已无活动 {}s",
                            last.elapsed().as_secs()
                        ));
                    }
                }
            }
        }
    }

    /// Request with retries and custom timeout.
    async fn request_with_retry(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout_ms: u64,
        max_retries: u32,
    ) -> anyhow::Result<serde_json::Value> {
        let mut last_err = None;
        for attempt in 0..=max_retries {
            let id = next_id();
            let (tx, rx) = oneshot::channel();
            {
                let mut pending = self.pending.lock().await;
                pending.insert(id, tx);
            }
            let msg = encode_request(id, method, params.clone());
            if let Some(stdin_tx) = &self.stdin_tx {
                stdin_tx.send(msg).await?;
            } else {
                anyhow::bail!("stdin closed");
            }

            let timeout = std::time::Duration::from_millis(timeout_ms);
            match tokio::time::timeout(timeout, rx).await {
                Ok(Ok(Ok(result))) => return Ok(result),
                Ok(Ok(Err(rpc_err))) => {
                    warn!(method, attempt, err = %rpc_err, "request failed");
                    last_err = Some(anyhow::anyhow!("RPC error: {rpc_err}"));
                }
                Ok(Err(_)) => {
                    last_err = Some(anyhow::anyhow!("request cancelled"));
                }
                Err(_) => {
                    self.pending.lock().await.remove(&id);
                    warn!(method, attempt, "request timeout");
                    last_err = Some(anyhow::anyhow!("timeout waiting for {method}"));
                }
            }
            if attempt < max_retries {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("{method} failed")))
    }

    async fn send_notification(&self, method: &str, params: serde_json::Value) {
        let msg = acp_protocol::encode_notification(method, params);
        if let Some(stdin_tx) = &self.stdin_tx {
            let _ = stdin_tx.send(msg).await;
        }
    }
}

/// Dispatch an incoming message from the agent's stdout
async fn dispatch_message(
    msg: RpcMessage,
    pending: &Arc<Mutex<HashMap<u64, PendingCallback>>>,
    event_tx: &mpsc::UnboundedSender<ClientEvent>,
    stdin_tx: &mpsc::Sender<String>,
    terminal_mgr: &Arc<TerminalManager>,
    adapter_name: &str,
    bus_tx: &Option<mpsc::UnboundedSender<BusEvent>>,
    agent_name: &str,
    cwd: &str,
) {
    if msg.is_response() {
        if let Some(id) = msg.id.as_ref().and_then(|v| v.as_u64()) {
            let mut pending = pending.lock().await;
            if let Some(cb) = pending.remove(&id) {
                if let Some(err) = msg.error {
                    let _ = cb.send(Err(err));
                } else {
                    let _ = cb.send(Ok(msg.result.unwrap_or(serde_json::json!({}))));
                }
            }
        }
    } else if msg.is_request() {
        handle_reverse_request(
            msg,
            stdin_tx,
            terminal_mgr,
            adapter_name,
            bus_tx,
            agent_name,
            cwd,
        )
        .await;
    } else if msg.is_notification() {
        handle_notification(msg, event_tx);
    }
}

/// Handle reverse requests from the agent (fs/*, terminal/*, session/request_permission)
async fn handle_reverse_request(
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
        "fs/read_text_file" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if path.is_empty() {
                encode_error(id, -32602, "missing path")
            } else {
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
                        Err(_) => {
                            encode_response(id, serde_json::json!({ "content": "" }))
                        }
                    },
                }
            }
        }
        "fs/write_text_file" => {
            let path = params.get("path").and_then(|v| v.as_str()).unwrap_or("");
            let content = params.get("content").and_then(|v| v.as_str());
            if path.is_empty() || content.is_none() {
                encode_error(id, -32602, "missing path or content")
            } else {
                match validate_path_within_cwd(path, cwd) {
                    Err(e) => encode_error(id, -32600, &format!("path outside working directory: {e}")),
                    Ok(resolved) => {
                        let content = content.unwrap();
                        if let Some(parent) = resolved.parent() {
                            let _ = tokio::fs::create_dir_all(parent).await;
                        }
                        match tokio::fs::write(&resolved, content).await {
                            Ok(()) => encode_response(id, serde_json::json!({})),
                            Err(e) => encode_error(id, -32000, &format!("cannot write: {}: {e}", resolved.display())),
                        }
                    }
                }
            }
        }
        "terminal/create" => {
            let result = terminal_mgr.handle_create(&params).await;
            match result {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/output" => {
            let tid = params
                .get("terminalId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match terminal_mgr.handle_output(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/wait_for_exit" => {
            let tid = params
                .get("terminalId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match terminal_mgr.handle_wait(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/kill" => {
            let tid = params
                .get("terminalId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match terminal_mgr.handle_kill(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "terminal/release" => {
            let tid = params
                .get("terminalId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match terminal_mgr.handle_release(tid).await {
                Ok(resp) => encode_response(id, resp),
                Err(e) => encode_error(id, -32000, &e.to_string()),
            }
        }
        "bus/send_message" => {
            let to = params.get("to").and_then(|v| v.as_str()).unwrap_or("");
            let content_text = params.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(tx) = bus_tx {
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
            } else {
                encode_error(id, -32000, "bus not available")
            }
        }
        "bus/list_agents" => {
            if let Some(tx) = bus_tx {
                let (reply_tx, reply_rx) = oneshot::channel();
                let _ = tx.send(BusEvent::ListAgents {
                    from_agent: agent_name.to_string(),
                    reply_tx,
                });
                match tokio::time::timeout(std::time::Duration::from_secs(5), reply_rx).await {
                    Ok(Ok(agents)) => {
                        let list: Vec<serde_json::Value> = agents.iter().map(|a| {
                            serde_json::json!({
                                "name": a.name,
                                "status": a.status,
                                "adapter": a.adapter,
                                "activity": a.activity,
                                "activeSecs": a.active_secs,
                            })
                        }).collect();
                        encode_response(id, serde_json::json!({"agents": list}))
                    }
                    _ => encode_error(id, -32000, "list_agents timeout"),
                }
            } else {
                encode_error(id, -32000, "bus not available")
            }
        }
        _ => {
            warn!(adapter = adapter_name, method, "unknown reverse request");
            encode_error(id, -32601, &format!("method not found: {method}"))
        }
    };

    let _ = stdin_tx.send(response).await;
}

/// Handle notifications from the agent
fn handle_notification(msg: RpcMessage, event_tx: &mpsc::UnboundedSender<ClientEvent>) {
    if msg.method.as_deref() == Some("session/update") {
        if let Some(params) = msg.params {
            let _ = event_tx.send(ClientEvent::SessionUpdate(params));
        }
    }
}

/// Check if a path is within the allowed working directory
/// Validate that a path is within the allowed working directory.
/// Returns the resolved absolute path on success.
fn validate_path_within_cwd(path: &str, cwd: &str) -> Result<std::path::PathBuf, String> {
    let cwd_canonical = match std::fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(_) => return Err("cannot resolve working directory".to_string()),
    };

    let target = std::path::Path::new(path);
    // Always resolve relative paths against cwd, not process CWD
    let abs_target = if target.is_absolute() {
        target.to_path_buf()
    } else {
        cwd_canonical.join(target)
    };

    // For existing paths, canonicalize directly
    // For non-existing paths (write), canonicalize the parent
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
