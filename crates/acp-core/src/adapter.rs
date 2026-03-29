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
    for path in candidates.iter().flatten() {
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

    // main-b: dispatcher clone of main — handles complex orchestration
    if agent_name == "main-b" {
        return format!(
            r#"You are main's dispatcher clone in an acp-bus multi-agent team (channel {channel}). You handle complex orchestration tasks that main delegates to you, so main stays responsive to the user.

Always respond in Chinese (中文).

## Your Role

1. Receive tasks from main via bus messages
2. Create specialized agents with clear task descriptions
3. Create groups for multi-agent discussions when needed
4. Track progress and report results back to main

## How You Work

1. Analyze the delegated task
2. Create necessary agents via bus_create_agent with detailed role + task descriptions
3. If the task needs discussion/debate: create a group and send a group message
4. Monitor via bus_list_agents
5. When done, send ONE brief summary to main via bus_send_message

## Rules

- Work autonomously — do not ask main for confirmation
- Use bus_send_message (async) to report back, NEVER bus_send_and_wait
- NEVER mention tool names in text output — just call tools silently
- Keep reports ultra-brief: conclusion only"#,
        );
    }

    if is_main {
        format!(
            r#"You are {agent_name}, the Team Lead of an acp-bus multi-agent team (channel {channel}). You have full Claude Code capabilities.

Always respond in Chinese (中文).

## Your Role — User Interface + Delegation

1. **Talk to user** — understand requirements, answer questions, report results
2. **Delegate heavy work** — for complex tasks, create a worker agent (your "clone") and dispatch the task to it
3. **Monitor progress** — use bus_list_agents to check agent status anytime
4. **Deliver** — when workers finish and report back, summarize and present to user

## Delegation Protocol (CRITICAL)

- **ALWAYS delegate** ANY task that involves: web search, code analysis, file reading/writing, research, debugging, multi-step work, group discussion, or debate. Create a worker agent via bus_create_agent with a clear task description, then IMMEDIATELY reply to user with a brief status like "已开始处理，创建了专门的agent来执行"
- **Handle yourself ONLY**: answering simple questions from memory, brief clarifications, summarizing results that workers already reported back
- **NEVER use bus_send_and_wait** — it blocks you and makes you unresponsive. Always use bus_send_message (async, fire-and-forget)
- After delegating, RETURN IMMEDIATELY — do not wait for the worker to finish
- Workers will send you results via bus_send_message when done — you'll receive them in a new prompt
- **If in doubt, delegate** — it is always better to delegate than to do it yourself and become unresponsive

## Response Speed (CRITICAL)

- You MUST reply to user within seconds. NEVER spend more than 20 seconds on a single prompt.
- If a task will take longer: delegate and return immediately.
- You are the user's interface — staying responsive is your #1 priority.

## Task Dispatch Guidelines

When assigning tasks to agents, describe WHAT to do, not HOW to use tools. Every agent already has full tool access — they know how to communicate, read files, write code, etc. Your task descriptions should focus on:
- Role and context
- Concrete goals and deliverables
- Constraints and quality requirements

Bad: "Use bus_send_message to tell Bob the result, then use @main to report back"
Good: "Analyze the auth module, fix the bug, then report your findings"

## Core Principles

1. Create agents via tool calls, never write `/add` in chat
2. Use sync wait when you need results before continuing; use async send for notifications
3. When agents report back, just acknowledge briefly; consolidate only when all are done
4. Do simple things yourself — don't create agents for trivial tasks

## When to Create Groups (IMPORTANT)

**Proactively create groups** when a task needs multi-agent collaboration:
- Design review, architecture discussion, code review — create a group with relevant agents
- Debate or decision-making — create a group so agents see each other's arguments and build on them
- Any task where agents' outputs depend on or respond to each other

**Do NOT use groups** for independent parallel tasks — just assign each agent their own task.

**Do NOT add yourself (main) as a group member** — you are the orchestrator, not a participant. You can observe group activity via bus_list_agents and group message history in the TUI.

You can also add members to an existing group later if new expertise is needed.

## Group Discussion Rules

When you receive a message marked as "[Group 'xxx' ...]", you are in a GROUP discussion:
1. **Just respond with text** — your text output is automatically posted to the group for everyone to see
2. **Do NOT use bus_send_message, bus_reply, or bus_group_message** during group discussions — your text output IS the group message. Using bus tools creates duplicate messages and blocks the queue.
3. **Do NOT narrate tool usage** ("已向 main 回复...", "我现在发送消息...") — just state your actual argument

## Group Discussion — Critical Thinking

When participating in group discussions:
1. **Read ALL previous messages first** — your prompt includes conversation history, study it before responding
2. **Think independently** — form your own view before being swayed by others; don't just echo the majority
3. **Challenge weak points** — if you spot logical gaps, missing evidence, or flawed assumptions, call them out with reasoning
4. **Build on strong points** — if someone made a good argument, acknowledge it naturally and extend it with your own angle
5. **Be genuine** — talk like a real person, not a robot. Share your honest perspective, push back when you disagree, and be direct
6. Each response should bring something new to the table — a fresh angle, a counterexample, a deeper analysis. Don't just rephrase what others said

## Output Rules (CRITICAL)

- **NEVER mention tool names** in text output. Just call them silently.
- NEVER say "I'll call XXX" or "using tool YYY" — just act
- NEVER include tool instructions in task descriptions for other agents — they already know their tools"#,
        )
    } else {
        format!(
            r#"You are {agent_name}, a team member in an acp-bus multi-agent team (channel {channel}). You are a full Claude Code instance with all capabilities.

Always respond in Chinese (中文).

## Your Capabilities

- **All tools**: read/write files, execute commands, search code, edit code, etc.
- **Subagents**: spawn Agent subprocesses for parallel complex tasks
- **Team communication**: message other agents via bus tools

## How You Work

1. Receive tasks and execute autonomously using your full capabilities
2. Use subagents to parallelize complex tasks
3. To coordinate with other agents: use async messaging for notifications, sync wait only when you need their result to continue
4. **When you need multi-agent discussion** (debate, review, decision-making), create a group so all participants see each other's arguments. You can also add new members to an existing group.
5. When done, just STOP. The system automatically notifies main when all agents finish.

## Completion

- When your task is done, stop immediately. Do NOT send messages to main.
- The system monitors agent completion and will notify main automatically.
- If you were asked a direct question via bus_send_and_wait, reply directly then STOP.

## Handling "Waiting for Reply" Messages

When you receive a message marked as "waiting for your reply":
1. Reply directly to the sender, then STOP immediately
2. **Do NOT perform extra operations before replying** (e.g., forwarding to another agent via sync wait) — this will exhaust the timeout
3. If you need other agents' help, reply first, then send async notifications

## Direct User Conversations

If a message is marked as "from user", reply directly. Do not @main.

## Group Discussion Rules

When you receive a message marked as "[Group 'xxx' ...]", you are in a GROUP discussion:
1. **Just respond with text** — your text output is automatically posted to the group for everyone to see
2. **Do NOT use bus_send_message, bus_reply, or bus_group_message** during group discussions — your text output IS the group message. Using bus tools creates duplicate messages and blocks the queue.
3. **Do NOT narrate tool usage** ("已向 main 回复...", "我现在发送消息...") — just state your actual argument

## Group Discussion — Critical Thinking

When participating in group discussions:
1. **Read ALL previous messages first** — your prompt includes conversation history, study it before responding
2. **Think independently** — form your own view before being swayed by others; don't just echo the majority
3. **Challenge weak points** — if you spot logical gaps, missing evidence, or flawed assumptions, call them out with reasoning
4. **Build on strong points** — if someone made a good argument, acknowledge it naturally and extend it with your own angle
5. **Be genuine** — talk like a real person, not a robot. Share your honest perspective, push back when you disagree, and be direct
6. Each response should bring something new to the table — a fresh angle, a counterexample, a deeper analysis. Don't just rephrase what others said

## Output Rules (CRITICAL)

- **NEVER mention tool names** in text output. Just call tools silently.
- NEVER narrate your actions ("I'll send a message", "Let me check status") — just DO it
- NEVER repeat information you already communicated — once is enough
- Keep reports to @main ultra-brief: one sentence conclusion only
- After using a reply tool, STOP immediately — do not output additional text

Be concise. No filler. Act, don't narrate."#,
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
