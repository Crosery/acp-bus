use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct AdapterConfig {
    pub name: String,
    pub description: String,
    pub cmd: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub terminal: bool,
    pub auth_method: Option<String>,
    pub auth_api_key: Option<String>,
    pub system_prompt: Option<String>,
    /// Tools to disallow via ACP _meta.claudeCode.options.disallowedTools
    pub disallowed_tools: Vec<String>,
    /// Path to the bus Unix socket (set at runtime, not by adapter definition)
    pub socket_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdapterDef {
    pub name: &'static str,
    pub description: &'static str,
    pub cmd: &'static str,
    pub args: &'static [&'static str],
    pub terminal: bool,
    pub auth_method: Option<&'static str>,
    pub env_keys: &'static [EnvMapping],
}

#[derive(Debug, Clone, Copy)]
pub struct EnvMapping {
    pub from: &'static str,
    pub to: &'static str,
}

static ADAPTERS: &[AdapterDef] = &[
    AdapterDef {
        name: "claude",
        description: "Claude Code (Anthropic)",
        cmd: "claude-agent-acp",
        args: &["--yolo"],
        terminal: true,
        auth_method: None,
        env_keys: &[],
    },
    AdapterDef {
        name: "c1",
        description: "Claude Code API1",
        cmd: "claude-agent-acp",
        args: &["--yolo"],
        terminal: true,
        auth_method: None,
        env_keys: &[
            EnvMapping {
                from: "CLAUDE_API1_BASE_URL",
                to: "ANTHROPIC_BASE_URL",
            },
            EnvMapping {
                from: "CLAUDE_API1_TOKEN",
                to: "ANTHROPIC_AUTH_TOKEN",
            },
        ],
    },
    AdapterDef {
        name: "c2",
        description: "Claude Code API2",
        cmd: "claude-agent-acp",
        args: &["--yolo"],
        terminal: true,
        auth_method: None,
        env_keys: &[
            EnvMapping {
                from: "CLAUDE_API2_BASE_URL",
                to: "ANTHROPIC_BASE_URL",
            },
            EnvMapping {
                from: "CLAUDE_API2_TOKEN",
                to: "ANTHROPIC_AUTH_TOKEN",
            },
        ],
    },
    AdapterDef {
        name: "gemini",
        description: "Gemini CLI (Google)",
        cmd: "gemini",
        args: &["--yolo", "--acp"],
        terminal: false,
        auth_method: Some("oauth-personal"),
        env_keys: &[],
    },
    AdapterDef {
        name: "codex",
        description: "Codex CLI (OpenAI)",
        cmd: "codex-acp",
        args: &[],
        terminal: false,
        auth_method: None,
        env_keys: &[EnvMapping {
            from: "OPENAI_API_KEY",
            to: "OPENAI_API_KEY",
        }],
    },
];

pub fn get_def(name: &str) -> Option<&'static AdapterDef> {
    ADAPTERS.iter().find(|a| a.name == name)
}

pub fn list() -> Vec<&'static str> {
    ADAPTERS.iter().map(|a| a.name).collect()
}

pub fn list_detailed() -> Vec<(&'static str, &'static str)> {
    ADAPTERS.iter().map(|a| (a.name, a.description)).collect()
}

/// Load .env file (KEY=VALUE lines, skip comments and empty lines)
fn load_dotenv() -> HashMap<String, String> {
    let mut vars = HashMap::new();
    // Try ~/.config/nvim/.env, then ~/.env
    let candidates = [
        dirs::config_dir().map(|d| d.join("nvim/.env")),
        dirs::home_dir().map(|d| d.join(".env")),
    ];
    for candidate in &candidates {
        if let Some(path) = candidate {
            if let Ok(content) = std::fs::read_to_string(path) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        continue;
                    }
                    if let Some((key, val)) = line.split_once('=') {
                        let val = val.trim().trim_matches('"');
                        vars.insert(key.trim().to_string(), val.to_string());
                    }
                }
                break; // Use first found
            }
        }
    }
    vars
}

/// Build a ready-to-use AdapterConfig with resolved environment variables.
pub fn get(name: &str, opts: &AdapterOpts) -> anyhow::Result<AdapterConfig> {
    let def = get_def(name).ok_or_else(|| anyhow::anyhow!("unknown adapter: {name}"))?;

    let mut env: HashMap<String, String> = HashMap::new();
    let dotenv = load_dotenv();

    // Resolve proxy env (check dotenv then system env)
    let proxy = dotenv
        .get("CLAUDE_PROXY")
        .cloned()
        .or_else(|| std::env::var("CLAUDE_PROXY").ok());
    if let Some(proxy) = proxy {
        if !proxy.is_empty() {
            for key in &[
                "http_proxy",
                "https_proxy",
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "all_proxy",
            ] {
                env.insert(key.to_string(), proxy.clone());
            }
        }
    }

    // Resolve adapter-specific env mappings (dotenv takes priority)
    for mapping in def.env_keys {
        let val = dotenv
            .get(mapping.from)
            .cloned()
            .or_else(|| std::env::var(mapping.from).ok());
        if let Some(val) = val {
            env.insert(mapping.to.to_string(), val);
        }
    }

    // Build system prompt
    let system_prompt = opts.agent_name.as_ref().map(|agent_name| {
        get_bus_system_prompt(agent_name, opts.channel_id.as_deref(), opts.is_main)
    });

    // Main agent: disallow built-in dispatch tools to force using /add + @mention
    let disallowed_tools = if opts.is_main {
        vec!["Agent".to_string(), "SendMessage".to_string()]
    } else {
        vec![]
    };

    Ok(AdapterConfig {
        name: name.to_string(),
        description: def.description.to_string(),
        cmd: def.cmd.to_string(),
        args: def.args.iter().map(|s| s.to_string()).collect(),
        env,
        terminal: def.terminal,
        auth_method: def.auth_method.map(|s| s.to_string()),
        auth_api_key: None,
        system_prompt,
        disallowed_tools,
        socket_path: None,
    })
}

#[derive(Debug, Clone, Default)]
pub struct AdapterOpts {
    pub bus_mode: bool,
    pub is_main: bool,
    pub agent_name: Option<String>,
    pub channel_id: Option<String>,
    pub cwd: Option<String>,
}

pub fn get_bus_system_prompt(agent_name: &str, channel_id: Option<&str>, is_main: bool) -> String {
    let channel = channel_id.unwrap_or("default");

    if is_main {
        format!(
            r#"你是 {agent_name}，运行在 acp-bus 多 agent 协作 TUI 中（频道 {channel}）。

你可以在回复文本中直接写命令来调度 agent，TUI 自动解析执行：
- /add <名字> <adapter> — 创建 agent（adapter: claude, c1, c2, gemini, codex）
- @<名字> <消息> — 给 agent 分发任务
- /remove <名字> — 移除 agent

工作流：用户发来任务 → 创建 worker（/add）→ 分发子任务（@mention，要求完成后 @{agent_name} 汇报）→ 收到汇报后整合结果回复用户。

示例回复：
/add w1 claude
/add w2 claude

然后下一轮：
@w1 调研 X，完成后 @{agent_name} 汇报
@w2 调研 Y，完成后 @{agent_name} 汇报

判断标准：可拆分的任务一律派发，只有单步简单操作才自己做。

你也可以直接使用 acp-bus MCP 工具：
- `bus_send_message` {{\"to\": \"agent_name\", \"content\": \"message\"}} — 像聊天一样直接私聊其他 agent，并拿到是否投递成功的回执
- `bus_list_agents` {{}} — 列出当前频道所有 agent 及状态

优先把 `bus_send_message` 当成“发私聊消息”的显式能力，不要把它理解成隐藏协议接口。"#,
        )
    } else {
        format!(
            r#"[SYSTEM — acp-bus]

你是 {agent_name}，acp-bus 频道 worker agent（频道 {channel}）。你是完整的 Claude Code 实例，有全部工具能力。

## 工作流

1. 收到任务 → 用全部工具能力完成（读写文件、执行命令、搜索等）
2. 完成后 → 回复里写 `@main 结果摘要`
3. 遇到需要协作的情况 → 主动用 `bus_send_message` 给其他 agent 发消息，不要把“能发消息”当成隐藏能力

直接干活，不废话。不要用 Neovim/nvim 相关功能（已废弃）。

可用 acp-bus MCP 工具：
- `bus_send_message` {{\"to\": \"agent_name\", \"content\": \"message\"}} — 给其他 agent 发消息，系统会记录 `from -> to`、投递结果和消息编号
- `bus_list_agents` {{}} — 列出频道 agent 及状态

如果你需要别人继续处理，直接发消息并明确要求对方完成后回复你或 `@main`，否则协作链路不会形成闭环。"#,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_adapters() {
        let names = list();
        assert!(names.contains(&"claude"));
        assert!(names.contains(&"gemini"));
        assert!(names.contains(&"codex"));
    }

    #[test]
    fn get_claude() {
        let config = get("claude", &AdapterOpts::default()).unwrap();
        assert_eq!(config.cmd, "claude-agent-acp");
        assert!(config.terminal);
        assert!(config.auth_method.is_none());
    }

    #[test]
    fn get_unknown() {
        assert!(get("nonexistent", &AdapterOpts::default()).is_err());
    }

    #[test]
    fn bus_system_prompt() {
        let opts = AdapterOpts {
            bus_mode: true,
            agent_name: Some("test-agent".into()),
            channel_id: Some("ch1".into()),
            ..Default::default()
        };
        let config = get("claude", &opts).unwrap();
        let prompt = config.system_prompt.unwrap();
        assert!(prompt.contains("test-agent"));
        assert!(prompt.contains("ch1"));
    }

    #[test]
    fn test_main_agent_disallowed_tools() {
        let opts = AdapterOpts {
            is_main: true,
            agent_name: Some("main".into()),
            ..Default::default()
        };
        let config = get("claude", &opts).unwrap();
        assert!(config.disallowed_tools.contains(&"Agent".to_string()));
        assert!(config.disallowed_tools.contains(&"SendMessage".to_string()));
        assert_eq!(config.disallowed_tools.len(), 2);
    }

    #[test]
    fn test_worker_agent_no_disallowed_tools() {
        let opts = AdapterOpts {
            is_main: false,
            agent_name: Some("w1".into()),
            ..Default::default()
        };
        let config = get("claude", &opts).unwrap();
        assert!(config.disallowed_tools.is_empty());
    }

    #[test]
    fn test_meta_construction() {
        // Reproduce the _meta building logic from client.rs
        let system_prompt = Some("you are a worker".to_string());
        let disallowed_tools = vec!["Agent".to_string(), "SendMessage".to_string()];

        let meta = {
            let mut meta = serde_json::Map::new();
            if let Some(ref sp) = system_prompt {
                meta.insert(
                    "systemPrompt".into(),
                    serde_json::json!({
                        "append": sp
                    }),
                );
            }
            if !disallowed_tools.is_empty() {
                meta.insert(
                    "claudeCode".into(),
                    serde_json::json!({
                        "options": {
                            "disallowedTools": disallowed_tools
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

        let meta = meta.unwrap();
        assert_eq!(meta["systemPrompt"]["append"], "you are a worker");
        assert_eq!(
            meta["claudeCode"]["options"]["disallowedTools"],
            serde_json::json!(["Agent", "SendMessage"])
        );
    }
}
