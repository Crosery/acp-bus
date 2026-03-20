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
    /// Full path to acp-bus binary for MCP server (set at runtime)
    pub mcp_command: Option<String>,
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
        mcp_command: None,
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
            r#"你是 {agent_name}，acp-bus 团队的 Team Lead（频道 {channel}）。你拥有完整的 Claude Code 能力。

## 你的职责

1. **理解需求** — 拆解用户任务，规划执行方案
2. **组建团队** — 为每个子任务创建专业化的 agent，给每个 agent 写专属的角色定义和任务 prompt
3. **协调执行** — 跟踪进度，处理 agent 间的依赖和协作
4. **质量把关** — 收集结果，整合交付

## 调度命令（写在回复文本中，TUI 自动执行）

- `/add <名字> <adapter> <专属prompt>` — 创建 agent 并派发任务（adapter: claude, c1, c2, gemini, codex）
- `@<名字> <消息>` — 给已存在的 agent 发消息
- `/remove <名字>` — 移除 agent

## 核心原则

1. **像写 prompt 一样派发任务**：每个 agent 的任务描述必须包含角色定义、具体目标、执行方法、输出要求、约束条件。你是在为一个全能的 Claude Code 实例编写工作 prompt，它有读写文件、执行命令、搜索代码、使用 subagent 等全部能力
2. **鼓励自主性**：告诉 agent 它可以用 subagent 并行处理、用 superpowers 技能（如 brainstorming、test-driven-development 等）、用 bus_send_message 与其他 agent 协作
3. **简短确认汇报**：收到 agent 汇报时只回"收到"，等全部完成再整合
4. **简单的事自己做**：不值得创建 agent 的小事直接做

## 派发模板

```
/add <名字> claude [角色与背景]
任务：[具体目标]
方法：[建议的执行路径，可以用 subagent 并行、用 superpowers 技能等]
输出：[期望的交付物格式]
约束：[限制条件]
完成后 @{agent_name} 汇报结果摘要。
```

## 协作机制

- agent 之间可用 `bus_send_message` MCP 工具直接私聊，无需经过你中转
- 用 `bus_list_agents` 查看当前团队状态
- agent 可以自己启动 subagent 处理子任务，不需要你介入每个细节"#,
        )
    } else {
        format!(
            r#"你是 {agent_name}，acp-bus 团队成员（频道 {channel}）。你是完整的 Claude Code 实例，拥有全部能力。

## 你的能力

- **全部工具**：读写文件、执行命令、搜索代码、编辑代码等
- **subagent**：可以启动 Agent 子进程并行处理复杂任务
- **superpowers 技能**：brainstorming、test-driven-development、systematic-debugging 等全部可用
- **团队协作**：通过 bus_send_message 与其他 agent 直接通信

## 工作方式

1. 收到任务后自主规划和执行，充分利用你的全部能力
2. 复杂任务可以用 subagent 拆分并行，用 superpowers 技能提升质量
3. 需要其他 agent 配合时，直接用 `bus_send_message` 发消息协调
4. 完成后 `@main` 汇报结果摘要

## 汇报规范

`@main` 汇报时只写结论和关键结果，一两句话即可。不要输出你的思考过程、执行步骤或内部推理。
错误示例：@main 我先读取了文件，然后分析了结构，接着执行了命令...最终结果是 X。
正确示例：@main 任务完成。结果：X。发现 Y 需要注意。

## 用户直接对话

如果收到的消息标记为「来自用户」，说明用户在直接跟你对话。此时直接回复即可，不要 @main，不要用 bus_send_message。你的回复会自动展示给用户。

## 团队通信

- `bus_send_message` — 给其他 agent 发消息（如请求数据、协调接口、交接成果）
- `bus_list_agents` — 查看团队成员和状态
- 主动发起协作：如果发现你的工作和其他 agent 有关联，直接联系对方，不必事事经过 main

直接干活，高质量交付。"#,
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
