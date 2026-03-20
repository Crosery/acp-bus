# acp-bus

多 AI Agent 协作总线 — 让 AI 组团干活。

acp-bus 是一个基于 ACP (Agent Communication Protocol) 的多 Agent 协作系统，通过 TUI 界面让用户以"团队管理"的方式调度多个 AI Agent 并行工作、互相通信、自主协作。

## 核心理念

**不是让一个 AI 更强，而是让多个 AI 像团队一样协作。**

- **Main Agent = Team Lead**：理解需求、拆解任务、组建团队、质量把关
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

TUI 内操作：

```
/add w1 claude 你的任务是...       # 创建 Agent 并派发任务
@w1 补充一下 X 部分               # 给已有 Agent 发消息
/remove w1                        # 移除 Agent
/list                             # 查看所有 Agent
/save                             # 保存会话快照
/cancel w1                        # 取消 Agent 当前任务
Ctrl+c                            # 退出（立即终止所有 Agent）
Ctrl+q                            # 中断选中 Agent 的当前任务
Ctrl+j/k                          # 上下滚动消息
Ctrl+d/u                          # 快速翻页（10行）
Ctrl+n/p                          # 切换 Agent 标签页
鼠标滚轮                          # 滚动消息
```

## 架构

```
┌─────────────────────────────────────────────────┐
│                    acp-bus TUI                  │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│  │ Main     │  │ Worker 1 │  │ Worker 2 │ ...   │
│  │ (Leader) │  │ (Claude) │  │ (Gemini) │       │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘       │
│       │              │              │           │
│  ┌────┴──────────────┴──────────────┴────┐      │
│  │         Bus (Unix Socket + MCP)        │     │
│  │  bus_send_message / bus_list_agents    │     │
│  └───────────────────────────────────────┘      │
│       │                                         │
│  ┌────┴──────┐  ┌──────────┐  ┌──────────┐      │
│  │ Scheduler │  │  Router  │  │  Store   │      │
│  │ (串行队列) │  │(@mention)│  │ (快照)   │     │
│  └───────────┘  └──────────┘  └──────────┘      │
└─────────────────────────────────────────────────┘
```

### Crate 结构

```
acp-protocol  ← 纯类型，JSON-RPC 2.0 + ACP 协议定义
acp-core      ← 核心逻辑：Channel、Agent、Router、Scheduler、Client、Store
acp-server    ← Neovim 集成（JSON-RPC over stdio）
acp-tui       ← ratatui TUI 界面
acp-bus-mcp   ← MCP Server（已内置到主二进制）
acp-mock      ← 测试用 Mock Agent
```

依赖方向：`acp-protocol ← acp-core ← acp-server / acp-tui ← main`

### 通信机制

**Agent ↔ Bus（ACP 协议）**

```
initialize → authenticate? → session/new → session/prompt
                                              ↓
                                        session/update (streaming)
                                              ↓
                                     reverse requests (fs/*, terminal/*)
```

**Agent ↔ Agent（MCP 工具）**

```
Agent A → bus_send_message(to: "B", content: "...") → Bus Socket → Channel → prompt Agent B
```

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

TUI 自动解析 → 创建 w1, w2 → 等待连接 → 派发任务（包含完整多行描述）
```

## 支持的 Adapter

| Adapter | 命令 | 说明 |
|---------|------|------|
| `claude` | claude-agent-acp | Claude Code (Anthropic) |
| `c1` | claude-agent-acp | Claude Code API 线路 1 |
| `c2` | claude-agent-acp | Claude Code API 线路 2 |
| `gemini` | gemini --yolo --acp | Gemini CLI (Google)，启动较慢（~25s），API 429 时静默无响应 |
| `codex` | codex-acp | Codex CLI (OpenAI) |

连通性测试：`cargo test -p acp-core --test real_agent_connectivity -- --ignored`

## 开发

```bash
cargo build                        # 构建
cargo test                         # 测试
cargo test -p acp-core             # 测试单个 crate
RUST_LOG=debug cargo run -- tui    # Debug 模式（日志输出到 stderr）
```

## Roadmap

### Phase 1：稳定性与可靠性 ✅ (当前)

- [x] ACP 协议握手 + 会话管理
- [x] MCP Server 内置化（解决 binary 发现问题）
- [x] Agent 间通信（bus_send_message / bus_list_agents）
- [x] 多行任务解析
- [x] 消息自动滚动（考虑 word wrap）
- [x] Scheduler 串行化 Main Agent 消息（防止消息风暴）
- [x] 原子写入、UTF-8 安全、进程管理修复
- [x] 强制终止：Ctrl+C 立即 SIGKILL 所有 Agent 进程组，不再卡死
- [x] 任务中断：Ctrl+Q 取消选中 Agent 的当前 prompt
- [x] 自发消息防护：禁止 Agent 通过 bus 给自己发消息（防死循环）
- [x] 用户直接对话：@worker 时 worker 直接回复，不再绕道 @main
- [x] Sidebar 工具调用实时展示（工具名 + 计时器）
- [x] 消息过滤：Agent tab 显示 @mention 该 Agent 的广播消息

### Phase 2：通信增强

- [ ] **消息投递确认**：Worker 收到消息后的 ACK 机制，确认对方真的收到并处理了
- [ ] **消息历史查询**：Agent 可以查询与特定 Agent 的历史对话
- [ ] **广播消息**：`bus_broadcast` 一对多通知，用于全局协调
- [ ] **消息优先级**：紧急消息插队，避免被队列阻塞
- [ ] **断线重连**：Agent 崩溃后自动重启并恢复会话上下文

### Phase 3：智能调度

- [ ] **动态负载均衡**：根据 Agent 忙闲自动分配任务
- [ ] **任务依赖图**：声明式定义任务间的依赖关系（A 完成后才启动 B）
- [ ] **超时与重试**：任务级别的超时监控和自动重试
- [ ] **Agent 池化**：预创建 Agent 池，避免每次创建的握手开销
- [ ] **成本追踪**：记录每个 Agent 的 token 消耗和 API 调用量

### Phase 4：可观测性

- [ ] **实时 Dashboard**：Agent 状态、消息流量、任务进度的可视化
- [ ] **通信拓扑图**：实时展示 Agent 间的消息流向
- [ ] **会话回放**：从快照回放历史会话，复盘协作过程
- [ ] **结构化日志**：OpenTelemetry 集成，分布式追踪

### Phase 5：生态集成

- [ ] **IDE 插件**：Neovim / VS Code 内嵌 Agent 面板
- [ ] **API 模式**：HTTP/WebSocket API，支持外部系统调用
- [ ] **Agent 模板市场**：预定义角色模板（代码审查员、测试工程师、文档撰写等）
- [ ] **多模型混合调度**：同一任务链中混用 Claude / Gemini / Codex，按特长分配
- [ ] **持久化频道**：频道持久化 + Agent 热加载，支持长期运行的项目频道

### Phase 6：高级协作模式

- [ ] **层级团队**：Team Lead 可以创建 Sub-Team Lead，形成树状管理结构
- [ ] **投票决策**：多个 Agent 对方案投票，少数服从多数
- [ ] **知识共享**：Agent 间共享发现的知识和中间结果，避免重复工作
- [ ] **代码冲突检测**：多个 Agent 同时修改代码时自动检测和解决冲突
- [ ] **Swarm 模式**：去中心化协作，无 Main Agent，Agent 自组织

## License

MIT
