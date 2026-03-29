# acp-bus

多 AI Agent 协作总线 — 让 AI 组团干活。

**[English](README.md) | 中文**

## 概览

acp-bus 是一个基于 ACP (Agent Communication Protocol) 的多 Agent 协作系统，通过 TUI 界面让用户以"团队管理"的方式调度多个 AI Agent 并行工作、互相通信、自主协作。

### 核心理念

**不是让一个 AI 更强，而是让多个 AI 像团队一样协作。**

- **Main Agent = 团队负责人**：理解需求、拆解任务、组建团队、质量把关
- **Worker Agent = 全能工程师**：拥有完整工具链，可自主使用 subagent、superpowers 技能、团队通信
- **Bus = 通信总线**：Agent 间通过 `bus_send_message` 直接点对点通信，无需经过 Main 中转

## 安装与运行

### 前置条件

- **Rust 工具链**：stable（见 `rust-toolchain.toml`）
- **至少一个 ACP Agent**：如 [Claude Code](https://claude.ai/code)（提供 `claude-agent-acp` 命令）

### 从源码构建

```bash
git clone https://github.com/crosery/acp-bus.git
cd acp-bus
cargo build --release
```

### 安装到系统

```bash
cargo install --path .
```

### 运行

```bash
# 方式一：cargo run
cargo run -- tui                       # 在当前目录启动 TUI
cargo run -- tui --cwd /your/project   # 指定工作目录

# 方式二：安装后直接运行
acp-bus tui
acp-bus tui --cwd /your/project

# 其他命令
acp-bus channels                       # 查看已保存的会话快照
acp-bus serve --stdio                  # JSON-RPC server 模式（Neovim 集成）
```

### 环境变量

```bash
RUST_LOG=debug acp-bus tui             # 开启 debug 日志（输出到 stderr）
```

如需使用多 API 线路或代理，在 `~/.env` 或 `~/.config/nvim/.env` 中配置：

```bash
# Claude API 线路
CLAUDE_API1_BASE_URL=https://your-api1.example.com
CLAUDE_API1_TOKEN=your-token

# 代理
CLAUDE_PROXY=http://proxy:port
```

## 操作指南

### 命令

```
/add w1 claude 你的任务是...       # 创建 Agent 并派发任务
@w1 补充一下 X 部分               # 给已有 Agent 发消息
/remove w1                        # 移除 Agent
/list                             # 查看所有 Agent
/adapters                         # 列出可用 Adapters
/group create debate A B C        # 创建群组（多方讨论）
/group add debate D               # 添加成员到群组
/group list                       # 列出所有群组
/group remove debate A            # 从群组移除成员
/save                             # 保存会话快照
/cancel w1                        # 取消 Agent 当前任务
/help                             # 帮助
/quit                             # 退出
```

### 快捷键

```
Ctrl+C                            # 退出（立即终止所有 Agent）
Ctrl+Q                            # 中断选中 Agent（System 标签时中断全部）
Ctrl+B                            # 收起/展开侧栏
Tab                               # 切换 私聊/群组 模式
Ctrl+N / Ctrl+P                   # 切换标签（补全弹出时上下选择）
Ctrl+J / Ctrl+K                   # 上下滚动消息
Ctrl+D / Ctrl+U                   # 快速翻页（10行）
Enter                             # 发送消息（补全弹出时确认选择）
Ctrl+Enter                        # 输入换行
Ctrl+V                            # 粘贴剪贴板图片
Esc                               # 关闭补全弹出
鼠标滚轮                          # 滚动消息
```

### 图片粘贴

复制图片到剪贴板后按 `Ctrl+V`，输入框会显示 `[Image-1]` 标记。可以多次粘贴叠加多张图片。Backspace 在标记上会整块删除并移除对应图片。支持 arboard 和 wl-paste/xclip 两种后端。

### 自动补全

输入 `/` 或 `@` 时自动弹出匹配列表，继续输入实时过滤。`Ctrl+N/P` 上下选择，`Enter` 确认，`Esc` 关闭。

## 架构

```
                         ┌─────────┐
                         │   You   │
                         └────┬────┘
                              │ @mention / 直接对话 / Ctrl+V 图片
                    ┌─────────┴─────────┐
                    │    acp-bus TUI     │
                    │  ┌──────┐ ┌─────┐ │
                    │  │侧边栏│ │消息区│ │
                    │  │·项目名│ │     │ │
                    │  │·Agent │ │     │ │
                    │  │·群组  │ │     │ │
                    │  │·快捷键│ │     │ │
                    │  └──┬───┘ └──┬──┘ │
                    │     │  输入框  │    │
                    └─────┼────────┼────┘
                          │        │
          ┌───────────────┼────────┼──────────────┐
          │            Router (@mention)           │
          └───┬───────────┼───────────────┬───────┘
              │           │               │
        ┌─────┴─────┐ ┌──┴────┐ ┌────────┴────┐
        │   Main    │ │Worker1│ │   Worker2    │
        │  (负责人) │ │(Claude│ │(Gemini/Codex)│
        │ ×1~5 弹性 │ │  c1)  │ │              │
        └─────┬─────┘ └──┬───┘ └──────┬───────┘
              │           │            │
              │     ACP Protocol (stdio JSON-RPC)
              │           │            │
        ┌─────┴───────────┴────────────┴───────┐
        │         Bus (Unix Socket + MCP)       │
        │                                       │
        │  bus_send_message   bus_send_and_wait  │
        │  bus_reply          bus_create_agent    │
        │  bus_group_message  bus_list_agents     │
        └──────────┬───────────────┬────────────┘
                   │               │
            ┌──────┴──────┐ ┌─────┴──┐
            │FairScheduler│ │  Store  │
            │(弹性 main   │ │(JSON   │
            │ 优先级队列)  │ │ 快照)   │
            └─────────────┘ └────────┘
```

### 通信机制

**Agent <-> Agent（MCP 工具）**

```
异步单向:   Agent A → bus_send_message(to: "B", ...) → Bus → prompt Agent B
同步等待:   Agent A → bus_send_and_wait(to: "B", ...) → Bus → prompt B → bus_reply → A 解除阻塞
群组讨论:   Agent A → bus_group_message(group: "debate") → Bus → 按顺序逐个 prompt 成员
```

**弹性 Main**：当 main 忙碌时，自动扩展 main-2, main-3...（最多 5 个实例），用同样的 adapter 和 system prompt。空闲实例优先复用，全忙时排队。

**死锁检测**：`bus_send_and_wait` 通过 `WaitGraph`（有向图环检测）防止 A 等 B 等 A 的死锁。

### 智能派发

```
用户: "帮我调研 X 和 Y"

Main Agent 回复:
/add w1 claude 你是技术调研专家。
任务：调研 X 的最新进展
输出：结构化报告
完成后 @main 汇报

/add w2 claude 你是市场分析师。
任务：分析 Y 的竞争格局
输出：对比表格
完成后 @main 汇报

TUI 自动解析 → 创建 w1, w2 → 等待连接 → 派发任务
```

## 支持的 Adapter

| Adapter | 命令 | 说明 |
|---------|------|------|
| `claude` | claude-agent-acp | Claude Code (Anthropic) |
| `c1` | claude-agent-acp | Claude Code API 线路 1 |
| `c2` | claude-agent-acp | Claude Code API 线路 2 |
| `gemini` | gemini --yolo --acp | Gemini CLI (Google)，启动较慢（~25s） |
| `codex` | codex-acp | Codex CLI (OpenAI) |

连通性测试：`cargo test -p acp-core --test real_agent_connectivity -- --ignored`

## 开发

```bash
cargo build                        # 构建
cargo test                         # 测试
cargo test -p acp-core             # 测试单个 crate
RUST_LOG=debug cargo run -- tui    # Debug 模式（日志输出到 stderr）
```

## License

MIT
