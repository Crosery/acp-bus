use serde::{Deserialize, Serialize};

// === Initialize ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeParams {
    pub protocol_version: u32,
    pub client_info: ClientInfo,
    pub client_capabilities: ClientCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fs: Option<FsCapabilities>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FsCapabilities {
    #[serde(default)]
    pub read_text_file: bool,
    #[serde(default)]
    pub write_text_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    pub protocol_version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_capabilities: Option<serde_json::Value>,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

// === Authenticate ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticateParams {
    pub method_id: String,
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

// === Session/New ===

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewParams {
    pub cwd: String,
    #[serde(default)]
    pub mcp_servers: serde_json::Value,
    /// ACP _meta field: supports systemPrompt, claudeCode.options, etc.
    #[serde(rename = "_meta", skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNewResult {
    pub session_id: String,
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl InitializeParams {
    pub fn default_with_terminal(terminal: bool) -> Self {
        Self {
            protocol_version: 1,
            client_info: ClientInfo {
                name: "acp-bus".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            client_capabilities: ClientCapabilities {
                fs: Some(FsCapabilities {
                    read_text_file: true,
                    write_text_file: true,
                }),
                terminal: if terminal { Some(true) } else { None },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_new_params_with_meta() {
        let params = SessionNewParams {
            cwd: "/tmp".into(),
            mcp_servers: serde_json::json!([]),
            meta: Some(serde_json::json!({
                "systemPrompt": { "append": "hello" }
            })),
        };
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["_meta"]["systemPrompt"]["append"], "hello");
        assert_eq!(json["cwd"], "/tmp");
    }

    #[test]
    fn test_session_new_params_without_meta() {
        let params = SessionNewParams {
            cwd: "/home".into(),
            mcp_servers: serde_json::json!([]),
            meta: None,
        };
        let json = serde_json::to_value(&params).unwrap();
        assert!(json.get("_meta").is_none());
    }

    #[test]
    fn test_initialize_params_with_terminal() {
        let params = InitializeParams::default_with_terminal(true);
        assert_eq!(params.client_capabilities.terminal, Some(true));
        let json = serde_json::to_value(&params).unwrap();
        assert_eq!(json["clientCapabilities"]["terminal"], true);
    }

    #[test]
    fn test_initialize_params_without_terminal() {
        let params = InitializeParams::default_with_terminal(false);
        assert!(params.client_capabilities.terminal.is_none());
        let json = serde_json::to_value(&params).unwrap();
        // terminal field should be skipped when None
        assert!(json["clientCapabilities"].get("terminal").is_none());
    }
}
