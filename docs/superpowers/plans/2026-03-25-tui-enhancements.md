# TUI 增强：无文本输出优化 + 思考装饰 + 实时状态标签

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消除 "(完成，无文本输出)" 噪音、实时显示 agent 思考内容（特殊装饰）、在 sidebar 显示中文状态标签。

**Architecture:** 三个独立改动，都围绕 `acp-core::agent::Agent` 和 `acp-tui` 展开。(1) Agent 新增 `has_bus_activity` 标志 + 提取纯函数 `should_show_empty_output` 供单元测试，完成时判断是否需要显示空输出提示；(2) Agent 新增 `thinking_buf` 收集思考文本，messages 渲染中以 `┊` 前缀 + 暗色斜体显示；(3) sidebar 新增 `status_label` 函数返回中文状态标签，利用已有 `activity` / `waiting_reply_from` / `status` 字段。

**Tech Stack:** Rust, ratatui, tokio, `#[cfg(test)]` 单元测试

**注意:** Task 1、2、3 必须串行执行（Task 1 和 2 都修改 `agent.rs` 和 `app.rs`，并行会导致合并冲突）。

---

## 文件结构

| 文件 | 职责 | 改动类型 |
|------|------|---------|
| `crates/acp-core/src/agent.rs` | Agent 结构体：新增 `thinking_buf`、`has_bus_activity`、`should_show_empty_output()` | 修改 |
| `crates/acp-tui/src/components/messages.rs` | 消息渲染：thinking 装饰 | 修改 |
| `crates/acp-tui/src/components/status_bar.rs` | sidebar：中文状态标签 | 修改 |
| `crates/acp-tui/src/theme.rs` | 新增 `THINKING_TEXT` 样式 | 修改 |
| `crates/acp-tui/src/app.rs` | session/update 收集 thinking、标记 bus_activity、完成逻辑 | 修改 |

---

## Task 1: Agent 新增 `has_bus_activity` 字段 + 消除空输出噪音

**Files:**
- Modify: `crates/acp-core/src/agent.rs`
- Modify: `crates/acp-tui/src/app.rs:1452` (完成逻辑)
- Modify: `crates/acp-tui/src/app.rs:247-293` (SendMessage 标记)
- Modify: `crates/acp-tui/src/app.rs:488-500` (Reply 标记)

### Step 1.1: RED — 写测试：Agent 的 `has_bus_activity` 字段 + `should_show_empty_output` 纯函数

在 `crates/acp-core/src/agent.rs` 的 `#[cfg(test)] mod tests` 中添加测试。

- [ ] **写测试**

```rust
// crates/acp-core/src/agent.rs — 在文件末尾追加 #[cfg(test)] mod tests 块内

#[test]
fn new_spawned_has_bus_activity_false() {
    let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    assert!(!agent.has_bus_activity);
}

#[test]
fn new_local_has_bus_activity_false() {
    let agent = Agent::new_local();
    assert!(!agent.has_bus_activity);
}

#[test]
fn reset_stream_clears_bus_activity() {
    let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    agent.has_bus_activity = true;
    agent.reset_stream();
    assert!(!agent.has_bus_activity);
}

#[test]
fn should_show_empty_output_true_when_no_activity() {
    let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    // stream_buf empty, no bus activity → should show
    assert!(agent.should_show_empty_output());
}

#[test]
fn should_show_empty_output_false_when_has_bus_activity() {
    let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    agent.has_bus_activity = true;
    assert!(!agent.should_show_empty_output());
}

#[test]
fn should_show_empty_output_false_when_has_stream_content() {
    let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    agent.stream_buf.push_str("some output");
    assert!(!agent.should_show_empty_output());
}
```

- [ ] **运行测试，确认 RED**

```bash
cargo test -p acp-core -- agent::tests::new_spawned_has_bus_activity_false agent::tests::should_show_empty_output 2>&1
```

预期：编译失败 — `has_bus_activity` 字段和 `should_show_empty_output` 方法不存在。

### Step 1.2: GREEN — 实现 `has_bus_activity` 字段 + `should_show_empty_output` 方法

- [ ] **在 `Agent` struct 中添加字段并初始化**

`crates/acp-core/src/agent.rs`:

在 `Agent` struct（第 46 行附近）的 `pending_task` 字段之后添加：

```rust
    /// Whether this agent made any bus tool calls during current prompt
    pub has_bus_activity: bool,
```

在 `new_spawned`（第 73 行附近）初始化列表中添加：

```rust
            has_bus_activity: false,
```

在 `new_local`（第 98 行附近）初始化列表中添加：

```rust
            has_bus_activity: false,
```

在 `reset_stream` 方法（第 147 行附近）中添加：

```rust
        self.has_bus_activity = false;
```

- [ ] **添加 `should_show_empty_output` 纯函数方法**

在 `Agent` 的 `impl` 块中（`reset_stream` 方法之后）添加：

```rust
    /// Whether the "(完成，无文本输出)" message should be shown.
    /// Returns false if agent produced text output or communicated via bus tools.
    pub fn should_show_empty_output(&self) -> bool {
        self.stream_buf.is_empty() && !self.has_bus_activity
    }
```

- [ ] **运行测试，确认 GREEN**

```bash
cargo test -p acp-core -- agent::tests::new_spawned_has_bus_activity agent::tests::new_local_has_bus_activity agent::tests::reset_stream_clears_bus agent::tests::should_show_empty_output 2>&1
```

预期：6 个测试 PASS。

- [ ] **运行全量测试确认无回归**

```bash
cargo test --workspace 2>&1
```

预期：全部 PASS。

### Step 1.3: 集成 app.rs — 标记 bus activity + 使用 `should_show_empty_output`

> **关键上下文：** `do_prompt` 和 `do_prompt_with_reply` 都是 `do_prompt_inner` 的薄包装（见 `app.rs:1184-1199`），"(完成，无文本输出)" **只有一处**（`do_prompt_inner` 约第 1454 行）。

- [ ] **在 `BusEvent::SendMessage` 的成功投递分支内标记 `has_bus_activity`**

`crates/acp-tui/src/app.rs` — 在第 247-293 行的 `else` 分支（成功投递分支）内，`post_directed_with_refs` 调用之后、`mark_waiting` 调用之前（约第 259 行），添加：

```rust
                        // Mark bus activity so "无文本输出" is suppressed
                        if let Some(agent) = ch.agents.get_mut(&from_agent) {
                            agent.has_bus_activity = true;
                        }
```

（此处 `ch` 已经是 `let mut ch = self.ctx.channel.lock().await` 锁住的，无需额外加锁。）

- [ ] **在 `BusEvent::Reply` 分支内标记 `has_bus_activity`**

`crates/acp-tui/src/app.rs` — 第 488-500 行的锁作用域内，`post_directed_with_refs` 之后添加：

```rust
                    // Mark bus activity
                    if let Some(agent) = ch.agents.get_mut(&from_agent) {
                        agent.has_bus_activity = true;
                    }
```

（同理，此处 `ch` 已经是 `mut` 锁住的。）

- [ ] **修改 `do_prompt_inner` 完成逻辑（唯一一处）**

`crates/acp-tui/src/app.rs` 约第 1452-1455 行，将：

```rust
            } else {
                let mut ch = ctx.channel.lock().await;
                ch.post(&name, "(完成，无文本输出)", true);
            }
```

改为：

```rust
            } else {
                let show_empty = {
                    let ch = ctx.channel.lock().await;
                    ch.agents.get(&name).map_or(true, |a| a.should_show_empty_output())
                };
                if show_empty {
                    let mut ch = ctx.channel.lock().await;
                    ch.post(&name, "(完成，无文本输出)", true);
                }
            }
```

- [ ] **在 `do_prompt_inner` 的 prompt 开始处重置 `has_bus_activity`**

`crates/acp-tui/src/app.rs` 约第 1230 行（`agent.streaming = true;` 附近），添加：

```rust
            agent.has_bus_activity = false;
```

- [ ] **REFACTOR — 当前无需重构，代码已足够简洁。**

- [ ] **编译 + 全量测试**

```bash
cargo build 2>&1 && cargo test --workspace 2>&1
```

预期：编译通过，全部测试 PASS。

- [ ] **Commit**

```bash
git add crates/acp-core/src/agent.rs crates/acp-tui/src/app.rs
git commit -m "fix: suppress '无文本输出' when agent communicated via bus tools"
```

---

## Task 2: Agent 新增 `thinking_buf` + 思考内容实时装饰显示

**Files:**
- Modify: `crates/acp-core/src/agent.rs`
- Modify: `crates/acp-tui/src/theme.rs`
- Modify: `crates/acp-tui/src/components/messages.rs`
- Modify: `crates/acp-tui/src/app.rs`

### Step 2.1: RED — 写测试：Agent 的 thinking_buf 基础行为

- [ ] **写测试**

```rust
// crates/acp-core/src/agent.rs — #[cfg(test)] mod tests 内追加
#[test]
fn new_spawned_has_empty_thinking_buf() {
    let agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    assert!(agent.thinking_buf.is_empty());
}

#[test]
fn reset_stream_clears_thinking_buf() {
    let mut agent = Agent::new_spawned("w1".into(), "claude".into(), None);
    agent.thinking_buf.push_str("some thinking");
    agent.reset_stream();
    assert!(agent.thinking_buf.is_empty());
}
```

- [ ] **运行测试，确认 RED**

```bash
cargo test -p acp-core -- agent::tests::new_spawned_has_empty_thinking_buf agent::tests::reset_stream_clears_thinking_buf 2>&1
```

预期：编译失败 — `thinking_buf` 字段不存在。

### Step 2.2: GREEN — 实现 `thinking_buf` 字段

- [ ] **在 `Agent` struct 中添加字段**

`crates/acp-core/src/agent.rs` — `Agent` struct 中，在 `stream_buf` 字段之后添加：

```rust
    /// Accumulated thinking text (from agent_thought_chunk events)
    pub thinking_buf: String,
```

在 `new_spawned` 和 `new_local` 的初始化列表中添加：

```rust
            thinking_buf: String::new(),
```

在 `reset_stream` 中添加：

```rust
        self.thinking_buf.clear();
```

- [ ] **运行测试，确认 GREEN**

```bash
cargo test -p acp-core -- agent::tests::new_spawned_has_empty_thinking_buf agent::tests::reset_stream_clears_thinking_buf 2>&1
```

预期：2 个测试 PASS。

- [ ] **REFACTOR — 当前无需重构。**

### Step 2.3: RED — 写测试：messages 渲染的 thinking 装饰

- [ ] **写测试**

```rust
// crates/acp-tui/src/components/messages.rs — 在文件末尾添加 #[cfg(test)] 模块
#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn format_thinking_line_adds_prefix() {
        let spans = format_thinking_line("让我分析一下");
        let text = spans_text(&spans);
        assert!(text.starts_with("┊ "));
        assert!(text.contains("让我分析一下"));
    }

    #[test]
    fn format_thinking_line_empty_input() {
        let spans = format_thinking_line("");
        let text = spans_text(&spans);
        assert_eq!(text, "┊ ");
    }

    #[test]
    fn format_thinking_line_preserves_content_exactly() {
        let spans = format_thinking_line("hello @world");
        let text = spans_text(&spans);
        // Thinking lines should NOT apply mention highlighting —
        // the text should be exactly as-is after the ┊ prefix
        assert_eq!(text, "┊ hello @world");
    }
}
```

- [ ] **运行测试，确认 RED**

```bash
cargo test -p acp-tui -- messages::tests 2>&1
```

预期：编译失败 — `format_thinking_line` 函数不存在。

### Step 2.4: GREEN — 实现 `format_thinking_line` + theme 样式

- [ ] **在 `theme.rs` 添加思考文本样式**

`crates/acp-tui/src/theme.rs` — 在 `STATUS_ERROR_BADGE` 之后、`status_icon` 函数之前添加：

```rust
pub const THINKING_PREFIX: Style = Style::new()
    .fg(Color::Rgb(120, 100, 160))
    .add_modifier(Modifier::DIM);
pub const THINKING_TEXT: Style = Style::new()
    .fg(Color::Rgb(140, 130, 170))
    .add_modifier(Modifier::ITALIC);
```

- [ ] **在 `messages.rs` 添加 `format_thinking_line` 函数**

`crates/acp-tui/src/components/messages.rs` — 在 `highlight_mentions` 函数之后添加：

```rust
/// Format a thinking line with ┊ prefix and dim italic style.
/// Unlike normal messages, thinking lines do NOT highlight @mentions.
fn format_thinking_line(text: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled("┊ ".to_string(), theme::THINKING_PREFIX),
        Span::styled(text.to_string(), theme::THINKING_TEXT),
    ]
}
```

- [ ] **运行测试，确认 GREEN**

```bash
cargo test -p acp-tui -- messages::tests 2>&1
```

预期：3 个测试 PASS。

- [ ] **REFACTOR — 当前无需重构。**

### Step 2.5: 集成 — streaming 预览中显示 thinking

- [ ] **`MessagesView` struct 添加 `thinking` 字段**

`crates/acp-tui/src/components/messages.rs` — `MessagesView` struct 中，在 `streaming` 字段之后添加：

```rust
    /// Live thinking previews: (agent_name, thinking_content)
    pub thinking: Vec<(String, String)>,
```

在 `MessagesView::new()` 中初始化：

```rust
            thinking: Vec::new(),
```

- [ ] **在 `build_text` 方法中渲染 thinking 预览**

在 `build_text` 方法中，现有 streaming 预览代码块之后（`// Append live streaming previews` 的 `for` 循环结束后），追加：

```rust
        // Append live thinking previews (only when not already streaming text)
        for (name, buf) in &self.thinking {
            if buf.is_empty() {
                continue;
            }
            // Skip if this agent already has a streaming preview
            if self.streaming.iter().any(|(n, b)| n == name && !b.is_empty()) {
                continue;
            }
            // Apply filter
            if let Some(ref f) = self.filter {
                if name != f {
                    continue;
                }
            }

            if !text.is_empty() {
                text.push(Line::from(""));
            }

            text.push(Line::from(vec![
                Span::styled(name.clone(), theme::AGENT_MSG.add_modifier(Modifier::BOLD)),
                Span::styled("  ...", Style::default().fg(Color::Rgb(120, 100, 160))),
            ]));
            // Show last 5 lines of thinking (avoid flooding the screen)
            let lines: Vec<&str> = buf.lines().collect();
            let start = lines.len().saturating_sub(5);
            for line in &lines[start..] {
                text.push(Line::from(format_thinking_line(line)));
            }
        }
```

- [ ] **在 `app.rs` 的 `collect_frame_data` 中收集 thinking 数据**

`crates/acp-tui/src/app.rs` — `collect_frame_data` 方法（约第 543 行）中：

在 `self.messages.streaming.clear();`（第 557 行）之后添加：

```rust
        self.messages.thinking.clear();
```

在现有 `if agent.streaming && !agent.stream_buf.is_empty()` 块（约第 576 行）之后添加：

```rust
            if !agent.thinking_buf.is_empty() {
                self.messages
                    .thinking
                    .push((agent.name.clone(), agent.thinking_buf.clone()));
            }
```

- [ ] **在 `app.rs` 的 session/update 事件处理中收集 thinking 文本**

两处 `agent_thought_chunk` 处理（约第 1097 行和第 1718 行），将：

```rust
Some("agent_thought_chunk") => {
    agent.activity = Some("thinking".into());
}
```

改为：

```rust
Some("agent_thought_chunk") => {
    if let Some(content) = update.get("content") {
        if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
            agent.thinking_buf.push_str(text);
        }
    }
    agent.activity = Some("thinking".into());
}
```

- [ ] **在 `do_prompt_inner` 的 prompt 开始处清空 thinking_buf**

`crates/acp-tui/src/app.rs` 约第 1231 行（`agent.stream_buf.clear()` 之后），添加：

```rust
            agent.thinking_buf.clear();
```

- [ ] **在 `do_prompt_inner` 的完成处清空 thinking_buf**

`crates/acp-tui/src/app.rs` 约第 1304 行（`std::mem::take(&mut agent.stream_buf)` 之后），添加：

```rust
            agent.thinking_buf.clear();
```

- [ ] **编译 + 全量测试**

```bash
cargo build 2>&1 && cargo test --workspace 2>&1
```

预期：编译通过，全部测试 PASS。

- [ ] **Commit**

```bash
git add crates/acp-core/src/agent.rs crates/acp-tui/src/theme.rs crates/acp-tui/src/components/messages.rs crates/acp-tui/src/app.rs
git commit -m "feat: display agent thinking content with ┊ prefix decoration in streaming preview"
```

---

## Task 3: Sidebar 实时中文状态标签

**Files:**
- Modify: `crates/acp-tui/src/components/status_bar.rs`

### Step 3.1: RED — 写测试：状态标签函数

- [ ] **写测试**

```rust
// crates/acp-tui/src/components/status_bar.rs — 文件末尾添加
#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent(status: &str, activity: Option<&str>, waiting: Option<&str>) -> AgentDisplay {
        AgentDisplay {
            name: "test".to_string(),
            status: status.to_string(),
            activity: activity.map(|s| s.to_string()),
            adapter: None,
            session_id: None,
            prompt_start_time: None,
            waiting_reply_from: waiting.map(|s| s.to_string()),
            waiting_since: None,
            waiting_conversation_id: None,
            tool_calls: Vec::new(),
        }
    }

    #[test]
    fn status_label_idle() {
        let agent = make_agent("idle", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "空闲");
    }

    #[test]
    fn status_label_thinking() {
        let agent = make_agent("streaming", Some("thinking"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "思考中");
    }

    #[test]
    fn status_label_typing() {
        let agent = make_agent("streaming", Some("typing"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "输出中");
    }

    #[test]
    fn status_label_tool() {
        let agent = make_agent("streaming", Some("Read"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "Read");
    }

    #[test]
    fn status_label_waiting() {
        let agent = make_agent("idle", None, Some("bob"));
        let (label, _) = status_label(&agent);
        assert_eq!(label, "等待 bob");
    }

    #[test]
    fn status_label_connecting() {
        let agent = make_agent("connecting", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "连接中");
    }

    #[test]
    fn status_label_error() {
        let agent = make_agent("error", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "错误");
    }

    #[test]
    fn status_label_disconnected() {
        let agent = make_agent("disconnected", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "断开");
    }

    #[test]
    fn status_label_receiving() {
        let agent = make_agent("streaming", Some("receiving"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "就绪");
    }
}
```

- [ ] **运行测试，确认 RED**

```bash
cargo test -p acp-tui -- status_bar::tests 2>&1
```

预期：编译失败 — `status_label` 函数不存在。

### Step 3.2: GREEN — 实现 `status_label` 函数

- [ ] **在 `status_bar.rs` 中添加 `status_label` 函数**

在 `status_char` 函数之后添加：

```rust
/// Return a Chinese status label and its style for display in sidebar.
fn status_label(agent: &AgentDisplay) -> (String, Style) {
    if agent.status == "error" {
        return ("错误".to_string(), Style::default().fg(Color::Red));
    }
    if agent.status == "disconnected" {
        return ("断开".to_string(), Style::default().fg(Color::DarkGray));
    }
    if agent.status == "connecting" {
        return ("连接中".to_string(), Style::default().fg(Color::Yellow));
    }
    if let Some(ref target) = agent.waiting_reply_from {
        return (
            format!("等待 {target}"),
            Style::default().fg(Color::Cyan),
        );
    }
    match agent.activity.as_deref() {
        Some("thinking") => ("思考中".to_string(), Style::default().fg(Color::Rgb(120, 100, 160))),
        Some("typing") => ("输出中".to_string(), Style::default().fg(Color::Green)),
        Some("receiving") => ("就绪".to_string(), Style::default().fg(Color::Yellow)),
        Some(tool_name) => (tool_name.to_string(), Style::default().fg(Color::Rgb(180, 160, 100))),
        None => ("空闲".to_string(), Style::default().fg(Color::Rgb(80, 90, 110))),
    }
}
```

- [ ] **运行测试，确认 GREEN**

```bash
cargo test -p acp-tui -- status_bar::tests 2>&1
```

预期：9 个测试全部 PASS。

### Step 3.3: REFACTOR — 将 `status_label` 集成到 sidebar 渲染，替换旧的 detail 逻辑

- [ ] **修改 `StatusBar::render` 方法**

在 `StatusBar::render` 中，删除原来的 detail 块（约第 113-155 行的 `let has_detail = ...` 到 `if has_detail && y < max_y { ... }` 整段），替换为以下代码（插入位置：agent 名行 `buf.set_line(inner.x, y, &Line::from(spans), w); y += 1;` 之后）：

```rust
            // Status label line (always shown for non-System agents)
            if agent.name != "System" && y < max_y {
                let (label, label_style) = status_label(agent);
                let mut detail = vec![Span::raw("     ")]; // indent

                // Timer (if actively prompting)
                if let Some(secs) = active_for_secs(agent) {
                    let t = if secs >= 60 {
                        format!("{}m{}s ", secs / 60, secs % 60)
                    } else {
                        format!("{secs}s ")
                    };
                    detail.push(Span::styled(t, Style::default().fg(Color::Yellow)));
                }

                detail.push(Span::styled(label, label_style));

                buf.set_line(inner.x, y, &Line::from(detail), w);
                y += 1;
            }
```

- [ ] **编译 + 全量测试**

```bash
cargo build 2>&1 && cargo test --workspace 2>&1
```

预期：编译通过，全部测试 PASS。

- [ ] **Commit**

```bash
git add crates/acp-tui/src/components/status_bar.rs
git commit -m "feat: show Chinese status labels in TUI sidebar (空闲/思考中/输出中/等待/连接中)"
```

---

## Task 4: 最终验证 + 清理

- [ ] **全量编译检查**

```bash
cargo clippy --workspace -- -D warnings 2>&1
```

- [ ] **全量测试**

```bash
cargo test --workspace 2>&1
```

- [ ] **格式化**

```bash
cargo fmt --all 2>&1
```

- [ ] **最终 Commit（如有 clippy/fmt 修复）**

```bash
git add -A && git commit -m "chore: clippy + fmt cleanup"
```

---

## 测试覆盖总结

| 测试位置 | 数量 | 覆盖内容 |
|---------|------|---------|
| `agent.rs` tests | 8 个 | `has_bus_activity` 初始/重置、`thinking_buf` 初始/重置、`should_show_empty_output` 三种场景 |
| `messages.rs` tests | 3 个 | `format_thinking_line` 正常/空/含@mention 场景 |
| `status_bar.rs` tests | 9 个 | `status_label` 全部 9 种状态分支 |
| **合计新增** | **20 个** | |

**已知测试缺口：** `app.rs` 中的集成逻辑（bus_activity 标记时机、thinking 收集时机）依赖运行时 ACP 协议交互，无法纯粹单元测试。通过以下方式缓解：
- 核心判断逻辑提取为 `Agent::should_show_empty_output()` 纯函数并有单元测试
- `format_thinking_line` 是纯函数并有单元测试
- `status_label` 是纯函数并有单元测试
- `app.rs` 的改动通过 `cargo build` + `cargo test --workspace` 保证编译和现有测试不回归

---

## 效果预期

### 改动前

```
  alice  02:56:58
  (完成，无文本输出)          ← 噪音

  系统  02:56:58
  alice 已完成
```

Sidebar:
```
 ▸ ● main
   ○ alice
   ○ bob
```

### 改动后

```
  系统  02:56:58               ← agent 通过 bus_reply 完成，无噪音
  alice 已完成
```

Thinking 实时预览:
```
  alice  ...
  ┊ 让我先分析代码结构...
  ┊ router.rs 的 @mention 逻辑需要检查
```

Sidebar:
```
 ▸ ● main
     12s 输出中
   ◎ alice
     等待 bob
   ● bob
     3s 思考中
   ○ carol
     空闲
```
