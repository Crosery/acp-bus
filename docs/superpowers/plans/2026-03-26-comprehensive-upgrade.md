# ACP-Bus 综合升级实施方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 全面升级 TUI 体验、添加群组协作、优化消息调度、加密提示词存储

**Architecture:** 分 4 个阶段实施。Phase 1 TUI 增强（侧栏收起、输入框美化、系统消息分类、agent 状态指示器、流式输出优化）；Phase 2 群组协作系统；Phase 3 消息调度优化（优先级队列 + 公平调度）；Phase 4 加密提示词存储 + 提示词优化

**Tech Stack:** Rust, ratatui 0.29, crossterm 0.28, tokio, aes-gcm (加密), sha2 (密钥派生), base64

**Dependencies:** Task 8 (FairScheduler) 依赖 Task 3 (SystemKind)。Phase 2-4 可独立于 Phase 1 实施，但 Task 8 必须在 Task 3 之后。

---

## 文件结构

### 新建文件
| 文件 | 职责 |
|------|------|
| `crates/acp-core/src/group.rs` | 群组模型 + 群组消息路由 |
| `crates/acp-core/src/prompt_store.rs` | 加密提示词加载/存储 |
| `crates/acp-core/src/fair_scheduler.rs` | 替代现有 Scheduler，支持优先级 + 公平调度 |

### 修改文件
| 文件 | 改动 |
|------|------|
| `crates/acp-tui/src/layout.rs` | 侧栏可收起 + 输入框区域重构 |
| `crates/acp-tui/src/theme.rs` | 新增系统消息子类样式、输入框样式、群组样式 |
| `crates/acp-tui/src/components/input.rs` | 输入框美化 + agent 状态指示器 |
| `crates/acp-tui/src/components/messages.rs` | 系统消息分类渲染 + 流式输出优化 |
| `crates/acp-tui/src/components/status_bar.rs` | 侧栏收起支持 + 群组显示 |
| `crates/acp-tui/src/app.rs` | 集成所有新功能、事件处理、群组命令 |
| `crates/acp-core/src/channel.rs` | 添加群组字段 + 群组消息 |
| `crates/acp-core/src/agent.rs` | 添加 group 归属字段 |
| `crates/acp-core/src/adapter.rs` | 改用 prompt_store 加载提示词 |
| `crates/acp-core/src/client.rs` | 新增 BusEvent 群组变体 |
| `crates/acp-core/src/bus_socket.rs` | 新增群组 socket 命令 |
| `crates/acp-bus-mcp/src/main.rs` | 新增群组 MCP 工具 |
| `crates/acp-core/src/lib.rs` | 导出 group、prompt_store、fair_scheduler |
| `crates/acp-core/Cargo.toml` | 添加 aes-gcm、sha2、base64、rand 依赖 |
| `crates/acp-tui/Cargo.toml` | （无需新增依赖） |

---

## Phase 1: TUI 增强

### Task 1: 侧栏可收起

**Files:**
- Modify: `crates/acp-tui/src/layout.rs`
- Modify: `crates/acp-tui/src/app.rs`
- Modify: `crates/acp-tui/src/components/status_bar.rs`

- [ ] **Step 1: 在 AppLayout 添加 sidebar_collapsed 参数**

```rust
// layout.rs - 整个文件重写
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub sidebar: Option<Rect>,  // None 时侧栏收起
    pub messages: Rect,
    pub input: Rect,
}

impl AppLayout {
    pub fn new(area: Rect, input_lines: u16, sidebar_collapsed: bool) -> Self {
        if sidebar_collapsed {
            // 无侧栏布局
            let max_input = (area.height / 3).max(2);
            let input_height = (input_lines + 1).clamp(2, max_input);
            let v_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(5),
                    Constraint::Length(input_height),
                ])
                .split(area);
            return Self {
                sidebar: None,
                messages: v_chunks[0],
                input: v_chunks[1],
            };
        }

        let sidebar_width = if area.width > 100 {
            24
        } else if area.width > 60 {
            20
        } else {
            16
        };

        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width),
                Constraint::Min(30),
            ])
            .split(area);

        let max_input = (h_chunks[1].height / 3).max(2);
        let input_height = (input_lines + 1).clamp(2, max_input);

        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                Constraint::Length(input_height),
            ])
            .split(h_chunks[1]);

        Self {
            sidebar: Some(h_chunks[0]),
            messages: v_chunks[0],
            input: v_chunks[1],
        }
    }
}
```

- [ ] **Step 2: 在 App 中添加 sidebar_collapsed 状态和 Ctrl+B 切换**

在 `App` struct 添加 `sidebar_collapsed: bool` 字段，在 `handle_key` 中添加 `Ctrl+B` 切换逻辑。

- [ ] **Step 3: 修改 draw() 中的布局计算**

```rust
// app.rs draw() 中
let layout = AppLayout::new(area, input_lines, self.sidebar_collapsed);
if let Some(sidebar_area) = layout.sidebar {
    self.status_bar.render(sidebar_area, &self.cached_agents, buf);
}
```

- [ ] **Step 4: 在 StatusBar::render 中显示收起提示**

当侧栏展开时底部显示 `Ctrl+B 收起`；收起时在消息区顶部显示一个最小化的 agent 状态条。

- [ ] **Step 5: 修改 draw() 中 sidebar_w 计算以响应 collapsed 状态**

```rust
let sidebar_w = if self.sidebar_collapsed { 0 } else { layout.sidebar.map(|s| s.width).unwrap_or(0) };
```

- [ ] **Step 6: 运行 `cargo build` 验证编译**

- [ ] **Step 6: 提交**
```bash
git add crates/acp-tui/src/layout.rs crates/acp-tui/src/app.rs crates/acp-tui/src/components/status_bar.rs
git commit -m "feat: 侧栏可收起，Ctrl+B 切换"
```

---

### Task 2: 输入框美化 + Agent 状态指示器

**Files:**
- Modify: `crates/acp-tui/src/components/input.rs`
- Modify: `crates/acp-tui/src/theme.rs`
- Modify: `crates/acp-tui/src/app.rs`

- [ ] **Step 1: 在 theme.rs 添加输入框相关样式**

```rust
// theme.rs 新增
pub const INPUT_BORDER: Style = Style::new().fg(Color::Rgb(60, 80, 120));
pub const INPUT_BORDER_FOCUS: Style = Style::new().fg(Color::Rgb(100, 140, 200));
pub const INPUT_PROMPT: Style = Style::new()
    .fg(Color::Rgb(100, 180, 255))
    .add_modifier(Modifier::BOLD);
pub const INPUT_STATUS_IDLE: Style = Style::new().fg(Color::Rgb(80, 100, 80));
pub const INPUT_STATUS_STREAMING: Style = Style::new().fg(Color::Yellow);
pub const INPUT_STATUS_THINKING: Style = Style::new().fg(Color::Rgb(140, 130, 170));
pub const INPUT_STATUS_ERROR: Style = Style::new().fg(Color::Red);
pub const INPUT_PLACEHOLDER: Style = Style::new().fg(Color::Rgb(70, 70, 80));
```

- [ ] **Step 2: 在 InputBox 添加 agent 状态显示数据**

```rust
// input.rs
pub struct InputBox {
    pub text: String,
    pub cursor_pos: usize,
    completions: Vec<String>,
    candidates: Vec<String>,
    selected: Option<usize>,
    popup_visible: bool,
    // 新增
    pub agent_name: Option<String>,
    pub agent_status: Option<String>,
    pub agent_activity: Option<String>,
    pub agent_adapter: Option<String>,
    pub active_secs: Option<i64>,
}
```

- [ ] **Step 3: 重写 render() 方法美化输入框**

```rust
pub fn render(&self, area: Rect, buf: &mut Buffer) {
    // 1. 上边框 + 状态信息栏
    let border_style = theme::INPUT_BORDER_FOCUS;
    let block = Block::default()
        .borders(Borders::TOP)
        .border_style(border_style);
    let inner = block.inner(area);
    block.render(area, buf);

    // 2. 在上边框右侧显示 agent 状态
    if let Some(ref name) = self.agent_name {
        let status_text = self.format_status_line(name);
        let (text, style) = status_text;
        let x = area.x + area.width.saturating_sub(text.width() as u16 + 2);
        buf.set_string(x, area.y, &text, style);
    }

    // 3. 提示符 + 文本
    let prompt = "❯ ";
    let prompt_w = 2u16; // ❯ 占 1 宽度 + 空格
    // ... 正常渲染文本

    // 4. 空输入时显示 placeholder
    if self.text.is_empty() {
        let placeholder = self.placeholder_text();
        buf.set_string(
            inner.x + prompt_w,
            inner.y,
            &placeholder,
            theme::INPUT_PLACEHOLDER,
        );
    }
}

fn format_status_line(&self, name: &str) -> (String, Style) {
    let status = self.agent_status.as_deref().unwrap_or("idle");
    let icon = match status {
        "streaming" => "●",
        "connecting" => "◌",
        "error" | "disconnected" => "✗",
        _ => "○",
    };
    let label = match self.agent_activity.as_deref() {
        Some("thinking") => "思考中",
        Some("typing") => "输出中",
        Some("receiving") => "接收中",
        Some(tool) => tool,
        None if status == "streaming" => "输出中",
        None => "空闲",
    };
    let elapsed = self.active_secs
        .map(|s| format!(" {s}s"))
        .unwrap_or_default();
    let text = format!("{icon} {name} · {label}{elapsed}");
    let style = match status {
        "streaming" => theme::INPUT_STATUS_STREAMING,
        "error" | "disconnected" => theme::INPUT_STATUS_ERROR,
        _ if self.agent_activity.is_some() => theme::INPUT_STATUS_THINKING,
        _ => theme::INPUT_STATUS_IDLE,
    };
    (text, style)
}

fn placeholder_text(&self) -> String {
    match self.agent_name.as_deref() {
        Some("system") | None => "输入消息… (Tab 补全, @agent 路由)".into(),
        Some(name) => format!("发送给 {name}… (Enter 发送)"),
    }
}
```

- [ ] **Step 4: 在 app.rs collect_frame_data() 中同步 agent 状态到 InputBox**

```rust
// 在 collect_frame_data() 末尾添加：
let selected_name = self.cached_agents.get(self.status_bar.selected)
    .map(|a| a.name.clone());
if let Some(ref name) = selected_name {
    let agent = self.cached_agents.iter().find(|a| &a.name == name);
    self.input.agent_name = Some(name.clone());
    if let Some(a) = agent {
        self.input.agent_status = Some(a.status.clone());
        self.input.agent_activity = a.activity.clone();
        self.input.agent_adapter = a.adapter.clone();
        self.input.active_secs = a.prompt_start_time
            .map(|t| (chrono::Utc::now().timestamp() - t).max(0));
    }
} else {
    self.input.agent_name = None;
}
```

- [ ] **Step 5: 运行 `cargo build` 验证**
- [ ] **Step 6: 提交**
```bash
git add crates/acp-tui/src/components/input.rs crates/acp-tui/src/theme.rs crates/acp-tui/src/app.rs
git commit -m "feat: 美化输入框，显示 agent 实时状态"
```

---

### Task 3: 系统消息分类显示

**Files:**
- Modify: `crates/acp-core/src/channel.rs`
- Modify: `crates/acp-tui/src/components/messages.rs`
- Modify: `crates/acp-tui/src/theme.rs`

- [ ] **Step 1: 在 channel.rs 添加 SystemKind 枚举**

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum SystemKind {
    AgentOnline,      // agent 上线
    AgentOffline,     // agent 离线/移除
    AgentComplete,    // agent 完成任务
    AgentError,       // agent 出错
    QueueNotice,      // 消息排队通知
    Routing,          // 消息路由信息
    General,          // 一般系统消息
}
```

在 `Message` struct 中添加 `pub system_kind: Option<SystemKind>`。

- [ ] **Step 2: 写测试验证 SystemKind 在消息中正确传递**

```rust
#[test]
fn test_system_kind_on_message() {
    let mut ch = Channel::new("/tmp".into());
    ch.post_system_typed("main 已上线", SystemKind::AgentOnline);
    let msg = ch.messages.last().unwrap();
    assert_eq!(msg.system_kind, Some(SystemKind::AgentOnline));
}
```

- [ ] **Step 3: 运行测试验证失败（post_system_typed 不存在）**
- [ ] **Step 4: 实现 post_system_typed 方法**

```rust
pub fn post_system_typed(&mut self, content: &str, kind: SystemKind) -> u64 {
    let id = self.post_system(content);
    if let Some(msg) = self.messages.last_mut() {
        msg.system_kind = Some(kind);
    }
    id
}
```

- [ ] **Step 5: 运行测试验证通过**
- [ ] **Step 6: 在 theme.rs 添加系统消息子类样式**

```rust
pub const SYSTEM_ONLINE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::ITALIC);
pub const SYSTEM_OFFLINE: Style = Style::new().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
pub const SYSTEM_COMPLETE: Style = Style::new().fg(Color::Rgb(80, 180, 80));
pub const SYSTEM_ERROR: Style = Style::new().fg(Color::Red);
pub const SYSTEM_QUEUE: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::ITALIC);
pub const SYSTEM_ROUTE: Style = Style::new().fg(Color::Rgb(100, 100, 140)).add_modifier(Modifier::DIM);
```

- [ ] **Step 7: 在 messages.rs build_text() 中根据 SystemKind 选择样式**

```rust
// 在 build_text() 中的系统消息处理部分
MessageKind::System => {
    let style = match &line.system_kind {
        Some(SystemKind::AgentOnline) => theme::SYSTEM_ONLINE,
        Some(SystemKind::AgentOffline) => theme::SYSTEM_OFFLINE,
        Some(SystemKind::AgentComplete) => theme::SYSTEM_COMPLETE,
        Some(SystemKind::AgentError) => theme::SYSTEM_ERROR,
        Some(SystemKind::QueueNotice) => theme::SYSTEM_QUEUE,
        Some(SystemKind::Routing) => theme::SYSTEM_ROUTE,
        _ => theme::SYSTEM_MSG,
    };
    // 添加前缀图标
    let prefix = match &line.system_kind {
        Some(SystemKind::AgentOnline) => "▲ ",
        Some(SystemKind::AgentOffline) => "▼ ",
        Some(SystemKind::AgentComplete) => "✓ ",
        Some(SystemKind::AgentError) => "✗ ",
        Some(SystemKind::QueueNotice) => "⏳ ",
        Some(SystemKind::Routing) => "→ ",
        _ => "· ",
    };
    spans.push(Span::styled(prefix, style));
    spans.push(Span::styled(&line.content, style));
}
```

- [ ] **Step 8: 在 app.rs 中所有 post 系统消息的地方替换为 typed 版本**

例如：
- `ch.post("系统", &format!("{name} 已上线"), true)` → `ch.post_system_typed(&format!("{name} 已上线"), SystemKind::AgentOnline)`
- `ch.post("系统", &format!("{name} 已完成"), true)` → `ch.post_system_typed(..., SystemKind::AgentComplete)`
- `ch.post("系统", &format!("{name} 出错: ..."), true)` → `ch.post_system_typed(..., SystemKind::AgentError)`
- 排队通知 → `SystemKind::QueueNotice`
- agent 退出 → `SystemKind::AgentOffline`

- [ ] **Step 9: 运行 `cargo test --workspace` 验证**
- [ ] **Step 10: 提交**
```bash
git commit -m "feat: 系统消息分类样式（上线/下线/完成/错误/排队/路由）"
```

---

### Task 4: 流式输出体验优化

**Files:**
- Modify: `crates/acp-tui/src/components/messages.rs`
- Modify: `crates/acp-tui/src/app.rs`

- [ ] **Step 1: 改善流式内容渲染 — 添加打字机光标和进度指示**

在 `messages.rs` 的流式预览渲染部分改进：

```rust
// 流式预览增强 — 替换当前的简单 "▌" 指示器
fn build_streaming_preview(name: &str, content: &str, elapsed_secs: Option<i64>) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // 头部：agent 名称 + 耗时
    let elapsed = elapsed_secs
        .map(|s| format!(" ({s}s)"))
        .unwrap_or_default();
    let header = Line::from(vec![
        Span::styled(
            format!("  {name}{elapsed} "),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ),
        Span::styled("━".repeat(20), Style::default().fg(Color::Rgb(60, 60, 40))),
    ]);
    lines.push(header);

    // 内容行
    for text_line in content.lines() {
        lines.push(Line::from(Span::styled(
            format!("  {text_line}"),
            Style::default().fg(Color::Rgb(200, 200, 180)),
        )));
    }

    // 末尾光标
    let last_line_content = content.lines().last().unwrap_or("");
    if !content.is_empty() {
        // 给最后一行加打字机光标
        if let Some(last) = lines.last_mut() {
            *last = Line::from(vec![
                Span::styled(
                    format!("  {last_line_content}"),
                    Style::default().fg(Color::Rgb(200, 200, 180)),
                ),
                Span::styled("▌", Style::default().fg(Color::Yellow)),
            ]);
        }
    }

    lines
}
```

- [ ] **Step 2: 减少流式刷新抖动 — 降低重绘频率**

在 `app.rs` 的事件循环中，将流式重绘间隔从 100ms 调整为自适应：

```rust
// 替换固定 100ms 重绘间隔
let redraw_interval = if has_streaming {
    Duration::from_millis(50)   // 流式输出时快速刷新
} else {
    Duration::from_millis(200)  // 空闲时慢速刷新
};
```

- [ ] **Step 3: 在 MessagesView 中传递 elapsed 信息给流式预览**

修改 `streaming` 字段类型为 `Vec<(String, String, Option<i64>)>` — 添加 elapsed_secs。

- [ ] **Step 3.1: 更新所有 streaming.push 调用点**

在 `app.rs` 中 grep 所有 `self.messages.streaming.push` 和 `messages.streaming =` 调用，将二元组改为三元组（添加 `agent.prompt_start_time.map(|t| (now - t).max(0))`）。

- [ ] **Step 4: 运行 `cargo build` 验证**
- [ ] **Step 5: 提交**
```bash
git commit -m "feat: 流式输出美化（打字机光标+进度+自适应刷新）"
```

---

## Phase 2: 群组协作系统

### Task 5: 群组核心模型

**Files:**
- Create: `crates/acp-core/src/group.rs`
- Modify: `crates/acp-core/src/lib.rs`
- Modify: `crates/acp-core/src/channel.rs`

- [ ] **Step 1: 写群组模型的测试**

```rust
// group.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_group() {
        let g = Group::new("research", "main");
        assert_eq!(g.name, "research");
        assert!(g.members.contains(&"main".to_string()));
    }

    #[test]
    fn add_and_remove_member() {
        let mut g = Group::new("team", "main");
        g.add_member("worker1");
        assert!(g.members.contains(&"worker1".to_string()));
        g.remove_member("worker1");
        assert!(!g.members.contains(&"worker1".to_string()));
    }

    #[test]
    fn cannot_remove_creator() {
        let mut g = Group::new("team", "main");
        assert!(!g.remove_member("main"));
    }

    #[test]
    fn list_other_members() {
        let mut g = Group::new("team", "main");
        g.add_member("w1");
        g.add_member("w2");
        let others = g.other_members("w1");
        assert!(others.contains(&&"main".to_string()));
        assert!(others.contains(&&"w2".to_string()));
        assert!(!others.contains(&&"w1".to_string()));
    }
}
```

- [ ] **Step 2: 运行测试验证失败**
- [ ] **Step 3: 实现 Group struct**

```rust
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Group {
    pub name: String,
    pub creator: String,
    pub members: HashSet<String>,
    pub created_at: i64,
}

impl Group {
    pub fn new(name: &str, creator: &str) -> Self {
        let mut members = HashSet::new();
        members.insert(creator.to_string());
        Self {
            name: name.to_string(),
            creator: creator.to_string(),
            members,
            created_at: chrono::Utc::now().timestamp(),
        }
    }

    pub fn add_member(&mut self, name: &str) {
        self.members.insert(name.to_string());
    }

    pub fn remove_member(&mut self, name: &str) -> bool {
        if name == self.creator {
            return false; // 创建者不能退出
        }
        self.members.remove(name)
    }

    pub fn is_member(&self, name: &str) -> bool {
        self.members.contains(name)
    }

    pub fn other_members(&self, exclude: &str) -> Vec<&String> {
        self.members.iter().filter(|m| m.as_str() != exclude).collect()
    }
}
```

- [ ] **Step 4: 运行测试验证通过**
- [ ] **Step 5: 在 lib.rs 添加 `pub mod group;`**
- [ ] **Step 6: 在 Channel 中添加群组注册表**

```rust
// channel.rs
use crate::group::Group;
use std::collections::HashMap;

pub struct Channel {
    // ... 现有字段 ...
    #[serde(default)]  // 向后兼容旧快照（没有 groups 字段）
    pub groups: HashMap<String, Group>,
}
```

- [ ] **Step 7: 写测试验证 Channel 的群组操作**

```rust
#[test]
fn test_create_group() {
    let mut ch = Channel::new("/tmp".into());
    ch.create_group("research", "main");
    assert!(ch.groups.contains_key("research"));
}

#[test]
fn test_group_message() {
    let mut ch = Channel::new("/tmp".into());
    ch.create_group("team", "main");
    ch.groups.get_mut("team").unwrap().add_member("w1");
    let recipients = ch.group_recipients("team", "main");
    assert_eq!(recipients, vec!["w1"]);
}
```

- [ ] **Step 8: 实现 Channel 群组方法**

```rust
impl Channel {
    pub fn create_group(&mut self, name: &str, creator: &str) -> bool {
        if self.groups.contains_key(name) {
            return false;
        }
        self.groups.insert(name.to_string(), Group::new(name, creator));
        self.post_system_typed(
            &format!("群组 [{name}] 已创建，创建者: {creator}"),
            SystemKind::General,
        );
        true
    }

    pub fn group_recipients(&self, group_name: &str, sender: &str) -> Vec<String> {
        self.groups
            .get(group_name)
            .map(|g| g.other_members(sender).into_iter().cloned().collect())
            .unwrap_or_default()
    }
}
```

- [ ] **Step 9: 运行 `cargo test -p acp-core` 验证**
- [ ] **Step 10: 提交**
```bash
git commit -m "feat: 群组核心模型和 Channel 集成"
```

---

### Task 6: 群组 Bus 通信 + MCP 工具

**Files:**
- Modify: `crates/acp-core/src/client.rs`
- Modify: `crates/acp-core/src/bus_socket.rs`
- Modify: `crates/acp-bus-mcp/src/main.rs`
- Modify: `crates/acp-tui/src/app.rs`

- [ ] **Step 1: 在 BusEvent 添加群组事件**

```rust
// client.rs BusEvent 新增变体
pub enum BusEvent {
    // ... 现有 ...
    CreateGroup {
        from_agent: String,
        name: String,
        members: Vec<String>,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    GroupMessage {
        from_agent: String,
        group_name: String,
        content: String,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
    JoinGroup {
        from_agent: String,
        group_name: String,
        reply_tx: oneshot::Sender<BusSendResult>,
    },
}
```

- [ ] **Step 2: 在 bus_socket.rs 添加群组命令处理**

```rust
// handle_line() 新增 match 分支
"create_group" => {
    let (reply_tx, reply_rx) = oneshot::channel();
    let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
    let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let members: Vec<String> = msg.get("members")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let _ = bus_tx.send(BusEvent::CreateGroup {
        from_agent: from.to_string(),
        name: name.to_string(),
        members,
        reply_tx,
    });
    // ... timeout + reply ...
}
"group_message" => {
    // 类似 send_message，但发给群组所有成员
}
"join_group" => {
    // 加入已有群组
}
```

- [ ] **Step 3: 在 acp-bus-mcp 添加 MCP 工具定义**

```rust
// main.rs tools 数组新增
{
    "name": "bus_create_group",
    "description": "创建讨论群组并邀请成员。群组内的消息会自动广播给所有成员。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "name": { "type": "string", "description": "群组名称" },
            "members": {
                "type": "array",
                "items": { "type": "string" },
                "description": "初始成员列表（agent 名称）"
            }
        },
        "required": ["name", "members"]
    }
},
{
    "name": "bus_group_message",
    "description": "向群组发送消息，所有群组成员都会收到。",
    "inputSchema": {
        "type": "object",
        "properties": {
            "group": { "type": "string", "description": "群组名称" },
            "content": { "type": "string", "description": "消息内容" }
        },
        "required": ["group", "content"]
    }
}
```

- [ ] **Step 4: 在 app.rs handle_bus_event 中处理群组事件**

```rust
BusEvent::CreateGroup { from_agent, name, members, reply_tx } => {
    let mut ch = self.ctx.channel.lock().await;
    if ch.create_group(&name, &from_agent) {
        for member in &members {
            if let Some(group) = ch.groups.get_mut(&name) {
                group.add_member(member);
            }
        }
        let _ = reply_tx.send(BusSendResult {
            message_id: None,
            delivered: true,
            error: None,
        });
    } else {
        let _ = reply_tx.send(BusSendResult {
            message_id: None,
            delivered: false,
            error: Some(format!("群组 {name} 已存在")),
        });
    }
}
BusEvent::GroupMessage { from_agent, group_name, content, reply_tx } => {
    let recipients = {
        let ch = self.ctx.channel.lock().await;
        ch.group_recipients(&group_name, &from_agent)
    };
    if recipients.is_empty() {
        let _ = reply_tx.send(BusSendResult {
            message_id: None,
            delivered: false,
            error: Some("群组不存在或无其他成员".into()),
        });
        return;
    }
    // 发送消息给每个成员
    {
        let mut ch = self.ctx.channel.lock().await;
        for recipient in &recipients {
            ch.post_directed(
                &from_agent,
                recipient,
                &format!("[群组 {group_name}] {content}"),
                MessageKind::Chat,
                MessageTransport::BusTool,
                MessageStatus::Delivered,
            );
        }
    }
    // 给每个成员派发 prompt（main 走调度器排队，worker 直接 spawn）
    for recipient in recipients {
        let ctx = self.ctx.clone();
        let msg = format!("[群组 {group_name} 消息，来自 {from_agent}]\n{content}");
        // main agent 必须经过调度器序列化，避免并发 prompt
        tokio::spawn(do_prompt(recipient, msg, ctx));
        // 注：do_prompt_inner 内部已有调度器 gate（name == "main" 分支），
        // 所以 do_prompt 对 main 已经走序列化路径，无需额外处理。
    }
    let _ = reply_tx.send(BusSendResult {
        message_id: None,
        delivered: true,
        error: None,
    });
}
```

- [ ] **Step 5: 运行 `cargo build` 验证**
- [ ] **Step 6: 提交**
```bash
git commit -m "feat: 群组通信 — bus_create_group + bus_group_message"
```

---

### Task 7: 群组 TUI 显示 + 命令

**Files:**
- Modify: `crates/acp-tui/src/app.rs`
- Modify: `crates/acp-tui/src/components/status_bar.rs`
- Modify: `crates/acp-tui/src/theme.rs`

- [ ] **Step 1: 在 theme.rs 添加群组样式**

```rust
pub const GROUP_NAME: Style = Style::new()
    .fg(Color::Rgb(180, 140, 255))
    .add_modifier(Modifier::BOLD);
pub const GROUP_BADGE: Style = Style::new()
    .fg(Color::Rgb(140, 100, 200));
```

- [ ] **Step 2: 在侧栏显示群组信息**

在 `StatusBar::render` 中，在 agent 列表下方显示群组列表：

```rust
// status_bar.rs render() 底部
// 显示群组
if !groups.is_empty() {
    // 分隔线
    buf.set_string(area.x + 1, y, "─ 群组 ─", theme::GROUP_BADGE);
    y += 1;
    for group in groups {
        let count = group.members.len();
        buf.set_string(
            area.x + 1, y,
            &format!("◈ {} ({})", group.name, count),
            theme::GROUP_NAME,
        );
        y += 1;
    }
}
```

- [ ] **Step 3: 在 input.rs COMMANDS 数组注册 /group 命令**

```rust
// input.rs
static COMMANDS: &[&str] = &[
    "/add", "/remove", "/list", "/adapters", "/cancel", "/help", "/quit", "/save",
    "/group",  // 新增
];
```

- [ ] **Step 4: 添加 /group 命令处理**

```rust
// app.rs handle_command() 新增
"/group" => {
    if parts.len() < 2 {
        ch.post("系统", "用法: /group create <name> <members...>\n       /group add <name> <member>\n       /group list", true);
        return;
    }
    match parts[1] {
        "create" => { /* 创建群组 */ }
        "add" => { /* 添加成员 */ }
        "list" => { /* 列出群组 */ }
        _ => { /* 未知子命令 */ }
    }
}
```

- [ ] **Step 4: 运行 `cargo build` 验证**
- [ ] **Step 5: 提交**
```bash
git commit -m "feat: 群组 TUI 显示 + /group 命令"
```

---

## Phase 3: 消息调度优化

### Task 8: 公平调度器替代简单队列

**Files:**
- Create: `crates/acp-core/src/fair_scheduler.rs`
- Modify: `crates/acp-core/src/lib.rs`
- Modify: `crates/acp-tui/src/app.rs`

- [ ] **Step 1: 写公平调度器的测试**

```rust
// fair_scheduler.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_between_senders() {
        let mut s = FairScheduler::new();
        // main is free, first message goes immediately
        assert!(s.enqueue("msg1", Some("w1"), Priority::Normal).unwrap());
        // main busy, queue messages from different senders
        assert!(!s.enqueue("msg2", Some("w2"), Priority::Normal).unwrap());
        assert!(!s.enqueue("msg3", Some("w1"), Priority::Normal).unwrap());
        assert!(!s.enqueue("msg4", Some("w3"), Priority::Normal).unwrap());

        // Drain: 尽量避免连续同一 sender
        // 队列 [w2, w1, w3]，last_sender=w1
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w2".to_string())); // != w1，选 w2
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w1".to_string())); // last=w2，w1 != w2，选 w1（队列第一个 != last）
        let next = s.drain().unwrap();
        assert_eq!(next.from, Some("w3".to_string())); // 剩余唯一
    }

    #[test]
    fn high_priority_first() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        s.enqueue("low", Some("w1"), Priority::Normal).unwrap();
        s.enqueue("urgent", Some("w2"), Priority::High).unwrap();

        let next = s.drain().unwrap();
        assert_eq!(next.content, "urgent"); // 高优先级先出
    }

    #[test]
    fn user_message_is_high_priority() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        s.enqueue("agent msg", Some("w1"), Priority::Normal).unwrap();
        s.enqueue("user msg", None, Priority::High).unwrap();

        let next = s.drain().unwrap();
        assert_eq!(next.content, "user msg"); // 用户消息优先
    }

    #[test]
    fn queue_full_rejects() {
        let mut s = FairScheduler::new();
        s.enqueue("first", None, Priority::Normal).unwrap();
        for i in 0..15 {
            let _ = s.enqueue(&format!("msg{i}"), Some("w1"), Priority::Normal);
        }
        assert!(s.enqueue("overflow", Some("w1"), Priority::Normal).is_err());
    }
}
```

- [ ] **Step 2: 运行测试验证失败**
- [ ] **Step 3: 实现 FairScheduler**

```rust
use std::collections::VecDeque;
use tracing::{info, warn};

const MAX_QUEUE: usize = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Normal = 0,
    High = 1,
}

#[derive(Debug, Clone)]
pub struct QueuedItem {
    pub content: String,
    pub from: Option<String>,
    pub reply_to: Option<String>,
    pub priority: Priority,
    pub enqueued_at: i64,
}

pub struct FairScheduler {
    queue: VecDeque<QueuedItem>,
    busy: bool,
    last_sender: Option<String>,
}

impl FairScheduler {
    pub fn new() -> Self {
        Self {
            queue: VecDeque::new(),
            busy: false,
            last_sender: None,
        }
    }

    pub fn enqueue(
        &mut self,
        content: &str,
        from: Option<&str>,
        priority: Priority,
    ) -> Result<bool, String> {
        self.enqueue_with_reply(content, from, None, priority)
    }

    pub fn enqueue_with_reply(
        &mut self,
        content: &str,
        from: Option<&str>,
        reply_to: Option<&str>,
        priority: Priority,
    ) -> Result<bool, String> {
        if !self.busy {
            self.busy = true;
            self.last_sender = from.map(String::from);
            return Ok(true); // 直接发送
        }
        if self.queue.len() >= MAX_QUEUE {
            warn!("scheduler queue full, dropping message");
            return Err(format!("队列已满（{MAX_QUEUE}），消息被丢弃"));
        }
        info!(from = ?from, "queuing message for main");
        self.queue.push_back(QueuedItem {
            content: content.to_string(),
            from: from.map(String::from),
            reply_to: reply_to.map(String::from),
            priority,
            enqueued_at: chrono::Utc::now().timestamp(),
        });
        Ok(false)
    }

    pub fn drain(&mut self) -> Option<QueuedItem> {
        self.busy = false;

        // 1. 高优先级优先
        if let Some(idx) = self.queue.iter().position(|q| q.priority == Priority::High) {
            self.busy = true;
            let item = self.queue.remove(idx).unwrap();
            self.last_sender = item.from.clone();
            return Some(item);
        }

        // 2. 公平轮转：选择与上一个发送者不同的消息
        if let Some(ref last) = self.last_sender {
            if let Some(idx) = self.queue.iter().position(|q| q.from.as_deref() != Some(last)) {
                self.busy = true;
                let item = self.queue.remove(idx).unwrap();
                self.last_sender = item.from.clone();
                return Some(item);
            }
        }

        // 3. 没有不同发送者，取第一个
        if let Some(item) = self.queue.pop_front() {
            self.busy = true;
            self.last_sender = item.from.clone();
            return Some(item);
        }

        None
    }

    pub fn is_busy(&self) -> bool {
        self.busy
    }

    pub fn queue_depth(&self) -> usize {
        self.queue.len()
    }
}

impl Default for FairScheduler {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4: 运行测试验证通过**
- [ ] **Step 5: 在 lib.rs 添加 `pub mod fair_scheduler;`**
- [ ] **Step 6: 在 app.rs 中用 FairScheduler 替换 Scheduler**

```rust
// app.rs
use acp_core::fair_scheduler::{FairScheduler, Priority};

// BusContext 中:
pub scheduler: Arc<Mutex<FairScheduler>>,

// do_prompt_inner 中的调度逻辑:
if name == "main" {
    let should_send = {
        let mut sched = ctx.scheduler.lock().await;
        let priority = if reply_to.is_none() {
            Priority::High  // 用户消息高优先级
        } else {
            Priority::Normal
        };
        match sched.enqueue_with_reply(
            &content,
            None, // from 在后续版本细化
            reply_to.as_deref(),
            priority,
        ) {
            Ok(immediate) => immediate,
            Err(msg) => {
                let mut ch = ctx.channel.lock().await;
                ch.post_system_typed(&msg, SystemKind::QueueNotice);
                return;
            }
        }
    };
    if !should_send {
        let depth = {
            let sched = ctx.scheduler.lock().await;
            sched.queue_depth()
        };
        let mut ch = ctx.channel.lock().await;
        ch.post_system_typed(
            &format!("main 忙碌中，消息已排队（队列: {depth}）"),
            SystemKind::QueueNotice,
        );
        return;
    }
}
```

- [ ] **Step 7: 在 drain 部分同步更新**

```rust
// do_prompt_inner 末尾
if name == "main" {
    let next = {
        let mut sched = ctx.scheduler.lock().await;
        sched.drain()
    };
    if let Some(queued) = next {
        let ctx2 = ctx.clone();
        if let Some(reply_to) = queued.reply_to {
            tokio::spawn(do_prompt_with_reply("main".into(), queued.content, ctx2, reply_to));
        } else {
            tokio::spawn(do_prompt("main".into(), queued.content, ctx2));
        }
    }
}
```

- [ ] **Step 8: 运行 `cargo test --workspace` 验证**
- [ ] **Step 9: 提交**
```bash
git commit -m "feat: 公平调度器（优先级+轮转），替代简单 FIFO 队列"
```

---

## Phase 4: 提示词优化 + 加密存储

### Task 9: 加密提示词存储

**Files:**
- Create: `crates/acp-core/src/prompt_store.rs`
- Modify: `crates/acp-core/Cargo.toml`
- Modify: `crates/acp-core/src/lib.rs`
- Modify: `crates/acp-core/src/adapter.rs`

- [ ] **Step 1: 添加加密依赖**

```toml
# crates/acp-core/Cargo.toml [dependencies] 新增
aes-gcm = "0.10"
sha2 = "0.10"
base64 = "0.22"
rand = "0.8"
```

- [ ] **Step 2: 写加密/解密测试**

```rust
// prompt_store.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip() {
        let key = derive_key("my-secret-passphrase");
        let plaintext = "你是一个AI助手";
        let encrypted = encrypt(plaintext, &key).unwrap();
        assert_ne!(encrypted, plaintext); // 确保已加密
        let decrypted = decrypt(&encrypted, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn wrong_key_fails() {
        let key1 = derive_key("correct-key");
        let key2 = derive_key("wrong-key");
        let encrypted = encrypt("secret", &key1).unwrap();
        assert!(decrypt(&encrypted, &key2).is_err());
    }

    #[test]
    fn save_and_load_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let store = PromptStore::new(dir.path().to_path_buf(), "test-key");

        store.save("main", "你是 Team Lead").unwrap();
        store.save("worker", "你是团队成员").unwrap();

        let main_prompt = store.load("main").unwrap();
        assert_eq!(main_prompt, "你是 Team Lead");

        let worker_prompt = store.load("worker").unwrap();
        assert_eq!(worker_prompt, "你是团队成员");
    }

    #[test]
    fn load_nonexistent_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let store = PromptStore::new(dir.path().to_path_buf(), "test-key");
        assert!(store.load("missing").is_err());
    }
}
```

- [ ] **Step 3: 运行测试验证失败**
- [ ] **Step 4: 实现 PromptStore**

```rust
use aes_gcm::{
    aead::{Aead, KeyInit, OsRng},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use std::path::PathBuf;

/// 从密码短语派生 256-bit 密钥（SHA-256）
pub fn derive_key(passphrase: &str) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let hash = Sha256::digest(passphrase.as_bytes());
    let mut key = [0u8; 32];
    key.copy_from_slice(&hash);
    key
}

pub fn encrypt(plaintext: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce_bytes: [u8; 12] = rand::random();
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|e| anyhow::anyhow!("encryption failed: {e}"))?;
    // 格式: base64(nonce + ciphertext)
    let mut combined = Vec::with_capacity(12 + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);
    Ok(B64.encode(&combined))
}

pub fn decrypt(encrypted: &str, key: &[u8; 32]) -> anyhow::Result<String> {
    let combined = B64.decode(encrypted)
        .map_err(|e| anyhow::anyhow!("base64 decode failed: {e}"))?;
    if combined.len() < 12 {
        anyhow::bail!("invalid encrypted data");
    }
    let (nonce_bytes, ciphertext) = combined.split_at(12);
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed (wrong key?): {e}"))?;
    Ok(String::from_utf8(plaintext)?)
}

pub struct PromptStore {
    dir: PathBuf,
    key: [u8; 32],
}

impl PromptStore {
    pub fn new(dir: PathBuf, passphrase: &str) -> Self {
        Self {
            dir,
            key: derive_key(passphrase),
        }
    }

    /// 默认存储路径: ~/.local/share/acp-bus/prompts/
    pub fn default_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("acp-bus")
            .join("prompts")
    }

    pub fn save(&self, name: &str, prompt: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let encrypted = encrypt(prompt, &self.key)?;
        let path = self.dir.join(format!("{name}.enc"));
        std::fs::write(path, encrypted)?;
        Ok(())
    }

    pub fn load(&self, name: &str) -> anyhow::Result<String> {
        let path = self.dir.join(format!("{name}.enc"));
        let encrypted = std::fs::read_to_string(&path)
            .map_err(|_| anyhow::anyhow!("prompt file not found: {name}"))?;
        decrypt(&encrypted, &self.key)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.dir.join(format!("{name}.enc")).exists()
    }
}
```

- [ ] **Step 5: 运行测试验证通过**

- [ ] **Step 6: 在 lib.rs 添加 `pub mod prompt_store;`**

- [ ] **Step 7: 添加 tempfile 为 dev-dependency**

```toml
# crates/acp-core/Cargo.toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 8: 运行 `cargo test -p acp-core prompt_store` 验证**
- [ ] **Step 9: 提交**
```bash
git commit -m "feat: 加密提示词存储（AES-256-GCM）"
```

---

### Task 10: 集成加密提示词到 adapter 系统

**Files:**
- Modify: `crates/acp-core/src/adapter.rs`

- [ ] **Step 1: 修改 adapter.rs 优先从加密存储加载提示词**

```rust
// adapter.rs
use crate::prompt_store::PromptStore;

pub fn get_bus_system_prompt(agent_name: &str, channel_id: Option<&str>, is_main: bool) -> String {
    // 1. 尝试从加密存储加载自定义提示词
    if let Ok(key) = std::env::var("ACP_PROMPT_KEY") {
        let store = PromptStore::new(PromptStore::default_dir(), &key);
        let prompt_name = if is_main { "main" } else { "worker" };
        if let Ok(custom) = store.load(prompt_name) {
            // 注入动态变量
            let channel = channel_id.unwrap_or("default");
            return custom
                .replace("{agent_name}", agent_name)
                .replace("{channel}", channel);
        }
    }

    // 2. 回退到内置提示词
    let channel = channel_id.unwrap_or("default");
    if is_main {
        get_default_main_prompt(agent_name, channel)
    } else {
        get_default_worker_prompt(agent_name, channel)
    }
}

// 将原有的硬编码提示词提取为独立函数（pub 可见性，供 CLI init-prompts 使用）
pub fn get_default_main_prompt(agent_name: &str, channel: &str) -> String {
    format!(r#"你是 {agent_name}，..."#) // 保持现有内容
}

pub fn get_default_worker_prompt(agent_name: &str, channel: &str) -> String {
    format!(r#"你是 {agent_name}，..."#) // 保持现有内容
}
```

- [ ] **Step 2: 添加 CLI 命令初始化加密提示词**

在 `src/main.rs` 的 `Commands` enum 添加 clap variant，然后在 match 中处理：

```rust
// Commands enum 新增 variant:
/// 初始化加密提示词存储
InitPrompts,

// main() match 新增分支:
Commands::InitPrompts => {
    let key = std::env::var("ACP_PROMPT_KEY")
        .expect("请设置 ACP_PROMPT_KEY 环境变量");
    let store = acp_core::prompt_store::PromptStore::new(
        acp_core::prompt_store::PromptStore::default_dir(),
        &key,
    );
    // 保存默认提示词的加密版本
    let main_prompt = acp_core::adapter::get_default_main_prompt("{agent_name}", "{channel}");
    let worker_prompt = acp_core::adapter::get_default_worker_prompt("{agent_name}", "{channel}");
    store.save("main", &main_prompt).expect("保存 main 提示词失败");
    store.save("worker", &worker_prompt).expect("保存 worker 提示词失败");
    println!("提示词已加密保存到: {:?}", acp_core::prompt_store::PromptStore::default_dir());
    println!("可以编辑加密文件来自定义提示词。");
}
```

- [ ] **Step 3: 运行 `cargo build` 验证**
- [ ] **Step 4: 提交**
```bash
git commit -m "feat: adapter 集成加密提示词，ACP_PROMPT_KEY 环境变量控制"
```

---

### Task 11: 优化提示词内容

**Files:**
- Modify: `crates/acp-core/src/adapter.rs`

- [ ] **Step 1: 优化 main agent 提示词 — 增强工具调用协调**

在现有 main prompt 基础上添加：

```
## 工具调用与对话协调

1. **先规划后行动**：收到复杂任务时，先用一条消息说明计划，再开始 tool call
2. **创建 agent 后立即派发**：bus_create_agent 的 task 参数必须包含完整任务描述，不要创建后再额外发消息
3. **批量操作**：如果需要创建多个 agent，在一次回复中连续调用 bus_create_agent，不要一个一个等
4. **状态感知**：在派发新任务前先 bus_list_agents 查看谁空闲
5. **群组协作**：需要多个 agent 讨论时，用 bus_create_group 建群，用 bus_group_message 发起讨论

## 群组使用指南

- `bus_create_group(name, members)` — 创建讨论群组
- `bus_group_message(group, content)` — 向群组发消息（所有成员收到）
- 适用场景：设计评审、方案讨论、多人协作
```

- [ ] **Step 2: 优化 worker agent 提示词 — 改善与工具的协调**

```
## 工具调用规范

1. **不要空转**：如果任务需要读文件，直接读，不要先说"我来读一下"
2. **批量操作**：多个独立操作可以并行调用工具
3. **错误恢复**：tool 报错后分析原因，调整参数重试，不要直接汇报错误
4. **结果导向**：汇报时只给结论和关键数据，不要描述执行过程

## 群组协作

- 收到群组消息时，结合上下文回复（你可以看到是哪个群组、谁发的）
- 群组讨论中可以 @其他成员发起直接对话
- 讨论结束后由发起人汇总结论给 @main
```

- [ ] **Step 3: 运行 `cargo test -p acp-core` 验证已有测试不受影响**
- [ ] **Step 4: 提交**
```bash
git commit -m "feat: 优化 main/worker 提示词，增强工具协调和群组协作指引"
```

---

## 验证清单

所有 Task 完成后：

- [ ] `cargo build` 编译通过
- [ ] `cargo test --workspace` 全部测试通过
- [ ] `cargo clippy --workspace` 无警告
- [ ] TUI 手动测试：
  - [ ] Ctrl+B 切换侧栏收起/展开
  - [ ] 输入框显示当前 agent 状态
  - [ ] 切换 agent tab 时输入框状态同步更新
  - [ ] 系统消息显示不同样式（上线绿色、错误红色、排队黄色等）
  - [ ] 流式输出有打字机光标效果
  - [ ] `/group create test w1 w2` 创建群组
  - [ ] 群组消息广播给所有成员
  - [ ] `ACP_PROMPT_KEY=xxx cargo run -- init-prompts` 加密存储提示词
  - [ ] 设置 `ACP_PROMPT_KEY` 后启动使用加密提示词
