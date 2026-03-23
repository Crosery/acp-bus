use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};
use tracing::info;

const DEFAULT_BYTE_LIMIT: u64 = 1024 * 1024;

struct Terminal {
    child: Option<Child>,
    pid: u32,
    output: Vec<u8>,
    output_bytes: u64,
    byte_limit: u64,
    truncated: bool,
    exited: bool,
    exit_code: Option<i32>,
    signal: Option<i32>,
    exit_notify: Arc<Notify>,
}

type Terminals = Arc<Mutex<HashMap<String, Terminal>>>;

pub struct TerminalManager {
    terminals: Terminals,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self {
            terminals: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn handle_create(
        &self,
        params: &serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        let cmd = params
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing command"))?;
        let args: Vec<String> = params
            .get("args")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        let cwd = params.get("cwd").and_then(|v| v.as_str());
        let byte_limit = params
            .get("outputByteLimit")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_BYTE_LIMIT);

        info!(cmd, "terminal/create");

        let mut command = Command::new(cmd);
        command
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }

        if let Some(env_map) = params.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env_map {
                if let Some(val) = v.as_str() {
                    command.env(k, val);
                }
            }
        }

        let mut child = command.spawn()?;
        let pid = child.id().unwrap_or(0);
        let tid = pid.to_string();

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let exit_notify = Arc::new(Notify::new());

        let terminal = Terminal {
            child: Some(child),
            pid,
            output: Vec::new(),
            output_bytes: 0,
            byte_limit,
            truncated: false,
            exited: false,
            exit_code: None,
            signal: None,
            exit_notify: exit_notify.clone(),
        };

        {
            self.terminals.lock().await.insert(tid.clone(), terminal);
        }

        // Collect stdout
        if let Some(mut stdout) = stdout {
            let terms = self.terminals.clone();
            let tid2 = tid.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut map = terms.lock().await;
                            if let Some(t) = map.get_mut(&tid2) {
                                if t.output_bytes < t.byte_limit {
                                    t.output.extend_from_slice(&buf[..n]);
                                    t.output_bytes += n as u64;
                                } else {
                                    t.truncated = true;
                                }
                            }
                        }
                    }
                }
            });
        }

        // Collect stderr (merged)
        if let Some(mut stderr) = stderr {
            let terms = self.terminals.clone();
            let tid2 = tid.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let mut map = terms.lock().await;
                            if let Some(t) = map.get_mut(&tid2) {
                                if t.output_bytes < t.byte_limit {
                                    t.output.extend_from_slice(&buf[..n]);
                                    t.output_bytes += n as u64;
                                } else {
                                    t.truncated = true;
                                }
                            }
                        }
                    }
                }
            });
        }

        // Wait for child exit in background
        {
            let terms = self.terminals.clone();
            let tid2 = tid.clone();
            tokio::spawn(async move {
                let mut child = {
                    let mut map = terms.lock().await;
                    map.get_mut(&tid2).and_then(|t| t.child.take())
                };
                if let Some(ref mut child) = child {
                    let status = child.wait().await;
                    let mut map = terms.lock().await;
                    if let Some(t) = map.get_mut(&tid2) {
                        t.exited = true;
                        if let Ok(status) = status {
                            t.exit_code = status.code();
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                t.signal = status.signal();
                            }
                        }
                        t.exit_notify.notify_waiters();
                    }
                }
            });
        }

        Ok(serde_json::json!({ "terminalId": tid }))
    }

    pub async fn handle_output(&self, tid: &str) -> anyhow::Result<serde_json::Value> {
        let map = self.terminals.lock().await;
        let t = map
            .get(tid)
            .ok_or_else(|| anyhow::anyhow!("unknown terminal: {tid}"))?;
        let output = String::from_utf8_lossy(&t.output).to_string();
        let mut resp = serde_json::json!({ "output": output, "truncated": t.truncated });
        if t.exited {
            resp["exitStatus"] = serde_json::json!({ "exitCode": t.exit_code, "signal": t.signal });
        }
        Ok(resp)
    }

    pub async fn handle_wait(&self, tid: &str) -> anyhow::Result<serde_json::Value> {
        let notify = {
            let map = self.terminals.lock().await;
            let t = map
                .get(tid)
                .ok_or_else(|| anyhow::anyhow!("unknown terminal: {tid}"))?;
            if t.exited {
                return Ok(serde_json::json!({ "exitCode": t.exit_code, "signal": t.signal }));
            }
            t.exit_notify.clone()
        };
        notify.notified().await;
        let map = self.terminals.lock().await;
        let t = map
            .get(tid)
            .ok_or_else(|| anyhow::anyhow!("unknown terminal: {tid}"))?;
        Ok(serde_json::json!({ "exitCode": t.exit_code, "signal": t.signal }))
    }

    pub async fn handle_kill(&self, tid: &str) -> anyhow::Result<serde_json::Value> {
        let map = self.terminals.lock().await;
        if let Some(t) = map.get(tid) {
            if !t.exited {
                #[cfg(unix)]
                if let Err(e) = crate::process::kill_process(t.pid, libc::SIGTERM) {
                    tracing::debug!(pid = t.pid, error = %e, "terminal kill failed");
                }
            }
        }
        Ok(serde_json::json!({}))
    }

    pub async fn handle_release(&self, tid: &str) -> anyhow::Result<serde_json::Value> {
        let mut map = self.terminals.lock().await;
        if let Some(t) = map.remove(tid) {
            if !t.exited {
                #[cfg(unix)]
                if let Err(e) = crate::process::kill_process(t.pid, libc::SIGTERM) {
                    tracing::debug!(pid = t.pid, error = %e, "terminal release kill failed");
                }
            }
        }
        Ok(serde_json::json!({}))
    }

    pub async fn cleanup(&self) {
        let mut map = self.terminals.lock().await;
        for (_, t) in map.drain() {
            if !t.exited {
                #[cfg(unix)]
                if let Err(e) = crate::process::kill_process(t.pid, libc::SIGTERM) {
                    tracing::debug!(pid = t.pid, error = %e, "terminal cleanup kill failed");
                }
            }
        }
    }
}
