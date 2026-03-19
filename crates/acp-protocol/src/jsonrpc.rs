use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

/// Standard JSON-RPC 2.0 error codes
pub mod error_codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for RpcError {}

/// A raw JSON-RPC 2.0 message (request, response, or notification)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcMessage {
    pub fn is_request(&self) -> bool {
        self.method.is_some() && self.id.is_some()
    }

    pub fn is_response(&self) -> bool {
        self.id.is_some() && (self.result.is_some() || self.error.is_some())
    }

    pub fn is_notification(&self) -> bool {
        self.method.is_some() && self.id.is_none()
    }
}

pub fn encode_request(id: u64, method: &str, params: serde_json::Value) -> String {
    serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    }))
    .unwrap()
}

pub fn encode_response(id: &serde_json::Value, result: serde_json::Value) -> String {
    serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    }))
    .unwrap()
}

pub fn encode_error(id: &serde_json::Value, code: i64, message: &str) -> String {
    serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
    .unwrap()
}

pub fn encode_notification(method: &str, params: serde_json::Value) -> String {
    serde_json::to_string(&serde_json::json!({
        "jsonrpc": "2.0",
        "method": method,
        "params": params,
    }))
    .unwrap()
}

/// Decode a JSON-RPC message from a line of text.
/// Skips non-JSON lines (lines that don't start with '{').
pub fn decode(line: &str) -> Option<RpcMessage> {
    let trimmed = line.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// Line-based buffer for streaming stdin parsing.
/// Accumulates partial data and yields complete lines on '\n'.
#[derive(Debug, Default)]
pub struct LineBuffer {
    buf: String,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of data, returns complete lines.
    pub fn feed(&mut self, chunk: &str) -> Vec<String> {
        if chunk.is_empty() {
            return Vec::new();
        }
        self.buf.push_str(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line = self.buf[..pos].trim_end_matches('\r').to_string();
            self.buf = self.buf[pos + 1..].to_string();
            if !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    pub fn reset(&mut self) {
        self.buf.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_buffer_partial() {
        let mut buf = LineBuffer::new();
        assert!(buf.feed("hello").is_empty());
        let lines = buf.feed(" world\n");
        assert_eq!(lines, vec!["hello world"]);
    }

    #[test]
    fn line_buffer_multi() {
        let mut buf = LineBuffer::new();
        let lines = buf.feed("a\nb\nc\n");
        assert_eq!(lines, vec!["a", "b", "c"]);
    }

    #[test]
    fn line_buffer_crlf() {
        let mut buf = LineBuffer::new();
        let lines = buf.feed("hello\r\n");
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn decode_request() {
        let msg = decode(r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{}}"#).unwrap();
        assert!(msg.is_request());
        assert!(!msg.is_response());
    }

    #[test]
    fn decode_response() {
        let msg = decode(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).unwrap();
        assert!(msg.is_response());
    }

    #[test]
    fn decode_notification() {
        let msg = decode(r#"{"jsonrpc":"2.0","method":"update","params":{}}"#).unwrap();
        assert!(msg.is_notification());
    }

    #[test]
    fn decode_non_json() {
        assert!(decode("not json").is_none());
        assert!(decode("").is_none());
    }

    #[test]
    fn id_generation() {
        let a = next_id();
        let b = next_id();
        assert!(b > a);
    }
}
