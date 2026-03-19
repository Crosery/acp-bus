use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tracing::debug;

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([a-zA-Z0-9_-]+)").unwrap());

const MAX_DEPTH: u32 = 5;

/// Parse @mentions from content, returning set of mentioned names (excluding sender).
pub fn parse_mentions(content: &str, from: &str) -> HashSet<String> {
    let mut mentioned = HashSet::new();
    for cap in MENTION_RE.captures_iter(content) {
        let name = cap[1].to_string();
        if name != from {
            mentioned.insert(name);
        }
    }
    mentioned
}

/// Route result: tells the scheduler who to send to.
#[derive(Debug)]
pub struct RouteTarget {
    pub name: String,
    pub is_main: bool,
    /// The content segment addressed to this agent.
    pub content: String,
}

/// Given a message, determine which agents should receive it.
/// Extracts per-agent content segments when multiple agents are mentioned.
pub fn route(content: &str, from: &str, known_agents: &[String], depth: u32) -> Vec<RouteTarget> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }

    let mentioned = parse_mentions(content, from);
    if mentioned.is_empty() {
        return Vec::new();
    }

    // Extract per-agent segments
    let segments = extract_segments(content, &mentioned);

    let mut targets = Vec::new();
    for name in &mentioned {
        if known_agents.contains(name) {
            let segment = segments.get(name.as_str()).cloned().unwrap_or_default();
            if segment.is_empty() {
                continue;
            }
            debug!(from, to = %name, "routing message");
            targets.push(RouteTarget {
                name: name.clone(),
                is_main: name == "main",
                content: segment,
            });
        }
    }

    targets
}

/// Extract per-agent content segments from a message.
///
/// If the message has lines starting with `@agent`, each agent gets only
/// the content from their `@agent` line(s). Context lines (without any
/// `@mention` to a routed agent) are prepended as shared context.
fn extract_segments(content: &str, mentioned: &HashSet<String>) -> HashMap<String, String> {
    let mut segments: HashMap<String, Vec<&str>> = HashMap::new();
    let mut context_lines: Vec<&str> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        // Check if this line starts with @agent for a mentioned agent
        let mut addressed_to: Option<&String> = None;
        for name in mentioned {
            if trimmed.starts_with(&format!("@{name}")) {
                addressed_to = Some(name);
                break;
            }
        }

        if let Some(name) = addressed_to {
            segments.entry(name.clone()).or_default().push(line);
        } else {
            // Skip command lines — agents don't need to see /add, /remove etc.
            if trimmed.starts_with('/') {
                continue;
            }
            context_lines.push(line);
        }
    }

    // If no per-agent lines found (e.g. all mentions are inline), give full content to everyone
    if segments.is_empty() {
        let mut result = HashMap::new();
        for name in mentioned {
            result.insert(name.clone(), content.to_string());
        }
        return result;
    }

    // Build final content: shared context + agent-specific lines
    let context = if context_lines.iter().all(|l| l.trim().is_empty()) {
        String::new()
    } else {
        let mut c = context_lines.join("\n").trim().to_string();
        if !c.is_empty() {
            c.push('\n');
        }
        c
    };

    let mut result = HashMap::new();
    for (name, lines) in &segments {
        let agent_content = lines.join("\n");
        if context.is_empty() {
            result.insert(name.clone(), agent_content);
        } else {
            result.insert(name.clone(), format!("{context}{agent_content}"));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_mention() {
        let m = parse_mentions("@claude-1 hello", "user");
        assert!(m.contains("claude-1"));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn parse_multiple_mentions() {
        let m = parse_mentions("@claude-1 @gemini-1 check this", "user");
        assert!(m.contains("claude-1"));
        assert!(m.contains("gemini-1"));
    }

    #[test]
    fn skip_self_mention() {
        let m = parse_mentions("@main hello @main", "main");
        assert!(m.is_empty());
    }

    #[test]
    fn route_to_known_agents() {
        let agents = vec!["main".into(), "claude-1".into()];
        let targets = route("@claude-1 do something", "main", &agents, 0);
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name, "claude-1");
        assert!(!targets[0].is_main);
        assert!(targets[0].content.contains("do something"));
    }

    #[test]
    fn route_depth_limit() {
        let agents = vec!["main".into()];
        let targets = route("@main hello", "other", &agents, 5);
        assert!(targets.is_empty());
    }

    #[test]
    fn route_per_agent_segments() {
        let agents = vec!["main".into(), "r1".into(), "r2".into(), "r3".into()];
        let content = "分发任务：\n@r1 调研 tokio\n@r2 调研 async-std\n@r3 调研 smol";
        let targets = route(content, "main", &agents, 0);
        assert_eq!(targets.len(), 3);

        let r1 = targets.iter().find(|t| t.name == "r1").unwrap();
        assert!(r1.content.contains("调研 tokio"));
        assert!(!r1.content.contains("调研 async-std"));
        assert!(!r1.content.contains("调研 smol"));

        let r2 = targets.iter().find(|t| t.name == "r2").unwrap();
        assert!(r2.content.contains("调研 async-std"));
        assert!(!r2.content.contains("调研 tokio"));
    }

    #[test]
    fn route_inline_mentions_give_full_content() {
        // When mentions are inline (not at line start), everyone gets full content
        let agents = vec!["main".into(), "r1".into(), "r2".into()];
        let content = "请 @r1 和 @r2 一起完成这个任务";
        let targets = route(content, "main", &agents, 0);
        assert_eq!(targets.len(), 2);
        for t in &targets {
            assert_eq!(t.content, content);
        }
    }

    #[test]
    fn route_context_lines_shared() {
        let agents = vec!["main".into(), "r1".into(), "r2".into()];
        let content = "背景信息：AI调研\n@r1 做任务A\n@r2 做任务B";
        let targets = route(content, "main", &agents, 0);

        let r1 = targets.iter().find(|t| t.name == "r1").unwrap();
        assert!(r1.content.contains("背景信息：AI调研"));
        assert!(r1.content.contains("做任务A"));
        assert!(!r1.content.contains("做任务B"));
    }
}
