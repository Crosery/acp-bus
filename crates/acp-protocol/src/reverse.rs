use serde::{Deserialize, Serialize};

// === fs/read_text_file ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsReadTextFileParams {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsReadTextFileResult {
    pub content: String,
}

// === fs/write_text_file ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsWriteTextFileParams {
    pub path: String,
    pub content: String,
}

// === terminal/create ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCreateParams {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_byte_limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCreateResult {
    pub terminal_id: String,
}

// === terminal/output ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputParams {
    pub terminal_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalOutputResult {
    pub output: String,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_status: Option<ExitStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExitStatus {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

// === terminal/wait_for_exit ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWaitParams {
    pub terminal_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalWaitResult {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
}

// === terminal/kill ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalKillParams {
    pub terminal_id: String,
}

// === terminal/release ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalReleaseParams {
    pub terminal_id: String,
}
