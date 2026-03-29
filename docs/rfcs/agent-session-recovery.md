# RFC: Agent Session Recovery

**Status**: Draft
**Author**: alice
**Date**: 2026-03-27

## 1. 动机

当前 acp-bus 中 agent 子进程崩溃或异常退出后，所有运行时状态（队列、等待关系、流式输出缓冲）全部丢失，用户需要手动重建 agent 并丢失上下文。对于长时间运行的多 agent 协作场景，这是核心可靠性短板。

**目标**：agent 崩溃后自动检测、重启进程、恢复状态，对用户透明。

## 2. 设计概览

```
┌─────────────────────────────────────────────────┐
│                  Recovery Flow                   │
│                                                  │
│  ClientEvent::Exited                             │
│       │                                          │
│       ▼                                          │
│  ┌──────────┐    ┌───────────┐    ┌───────────┐ │
│  │ Detect   │───▶│ Load      │───▶│ Re-spawn  │ │
│  │ Crash    │    │ Checkpoint│    │ Process   │ │
│  └──────────┘    └───────────┘    └───────────┘ │
│                                        │         │
│                                        ▼         │
│                  ┌───────────┐    ┌───────────┐ │
│                  │ Restore   │◀───│ Handshake │ │
│                  │ State     │    │ + Inject  │ │
│                  └───────────┘    └───────────┘ │
│                       │                          │
│                       ▼                          │
│                  ┌───────────┐                   │
│                  │ Resume    │                   │
│                  │ Execution │                   │
│                  └───────────┘                   │
└─────────────────────────────────────────────────┘
```

三层机制：
1. **Checkpoint**：定期序列化可恢复状态到磁盘
2. **Crash Detection**：监听 `ClientEvent::Exited`，区分正常退出和异常崩溃
3. **Recovery**：重启进程、执行握手、注入历史上下文、恢复队列和等待关系

## 3. Checkpoint 机制

### 3.1 触发时机

| 事件 | 触发 Checkpoint |
|------|----------------|
| Agent 完成一次 prompt（stopReason 到达） | ✓ |
| bus_reply 完成 | ✓ |
| Agent 状态变更（Idle ↔ Streaming） | ✓ |
| 定时（每 60 秒） | ✓ |
| Channel 手动 /save | ✓ |

### 3.2 序列化格式

```rust
#[derive(Serialize, Deserialize)]
pub struct Checkpoint {
    pub version: u32,
    pub timestamp: i64,
    pub channel_id: String,

    /// 每个 agent 的可恢复状态
    pub agents: HashMap<String, AgentCheckpoint>,

    /// FairScheduler 队列快照
    pub scheduler: SchedulerCheckpoint,

    /// WaitGraph 边集合
    pub wait_edges: HashMap<String, String>,

    /// 群组状态
    pub groups: HashMap<String, GroupCheckpoint>,
}

#[derive(Serialize, Deserialize)]
pub struct AgentCheckpoint {
    pub adapter_name: String,
    pub session_id: Option<String>,

    // Bus 通信状态
    pub waiting_reply_from: Option<String>,
    pub waiting_since: Option<i64>,
    pub waiting_conversation_id: Option<u64>,
    pub last_closed_conversation_id: Option<u64>,

    // 任务状态
    pub pending_task: Option<String>,
    pub current_task: Option<String>,

    // 最近 N 条该 agent 参与的消息摘要（用于上下文注入）
    pub recent_context: Vec<ContextMessage>,
}

#[derive(Serialize, Deserialize)]
pub struct ContextMessage {
    pub from: String,
    pub content_summary: String,  // 截断到 500 字符
    pub timestamp: i64,
}

#[derive(Serialize, Deserialize)]
pub struct SchedulerCheckpoint {
    pub busy: bool,
    pub last_sender: Option<String>,
    pub queue: Vec<QueuedItemCheckpoint>,
}

#[derive(Serialize, Deserialize)]
pub struct QueuedItemCheckpoint {
    pub content: String,
    pub from: Option<String>,
    pub reply_to: Option<String>,
    pub priority: String,  // "high" | "normal"
}

#[derive(Serialize, Deserialize)]
pub struct GroupCheckpoint {
    pub name: String,
    pub creator: String,
    pub members: Vec<String>,
}
```

### 3.3 存储路径

```
~/.local/share/acp-bus/channels/{encoded_cwd}/{channel_id}.checkpoint.json
```

与现有 Snapshot 同目录，独立文件。Checkpoint 写入使用 **原子写**（先写 `.tmp` 再 rename），防止写入中途崩溃导致 checkpoint 损坏。

### 3.4 大小控制

- `recent_context` 最多保留最近 20 条消息，每条摘要截断 500 字符
- 完整消息历史仍由 Snapshot 持久化，Checkpoint 只保留恢复所需的最小状态
- 预估大小：5 agent × 20 条 × 500 字 ≈ 50KB，可接受

## 4. 崩溃检测与分类

### 4.1 退出事件处理

```rust
// channel.rs — 现有 ClientEvent::Exited 处理扩展
match event {
    ClientEvent::Exited { code } => {
        let crash_type = classify_exit(code, &agent);
        match crash_type {
            ExitType::Normal => {
                // 正常完成，清理状态
                agent.status = AgentStatus::Disconnected;
            }
            ExitType::Crash { recoverable } if recoverable => {
                // 异常退出，尝试恢复
                self.attempt_recovery(&agent_name).await;
            }
            ExitType::Crash { .. } => {
                // 不可恢复，通知用户
                self.post_system(SystemKind::AgentError,
                    format!("Agent {} crashed (exit {}), recovery failed", name, code));
            }
        }
    }
}
```

### 4.2 退出分类

```rust
pub enum ExitType {
    /// 正常退出（code 0，或用户主动 /cancel）
    Normal,
    /// 异常退出
    Crash {
        /// 是否值得尝试恢复
        recoverable: bool,
    },
}

fn classify_exit(code: Option<i32>, agent: &Agent) -> ExitType {
    match code {
        Some(0) => ExitType::Normal,
        // 被 SIGKILL（137）或 SIGTERM（143）但非用户主动取消
        Some(137) | Some(143) if !agent.cancel_requested => {
            ExitType::Crash { recoverable: true }
        }
        // OOM (137) 或其他信号
        Some(_) => ExitType::Crash { recoverable: true },
        // 未知退出码
        None => ExitType::Crash { recoverable: true },
    }
}
```

### 4.3 恢复次数限制

每个 agent 维护恢复计数器，防止无限重启循环：

```rust
pub struct RecoveryState {
    pub attempts: u32,
    pub last_attempt: Option<i64>,
}

const MAX_RECOVERY_ATTEMPTS: u32 = 3;
const RECOVERY_COOLDOWN_SECS: i64 = 30;
```

- 最多连续恢复 3 次
- 两次恢复间隔至少 30 秒
- 超过限制后标记 agent 为 `AgentStatus::Error`，通知用户手动干预
- 成功运行 5 分钟后重置计数器

## 5. 恢复流程

### 5.1 完整流程

```
1. 加载 Checkpoint
2. 重新 spawn 子进程（同原 adapter 配置）
3. 执行握手：initialize → authenticate → session/new
4. 注入上下文摘要（通过 session/prompt 的 system message）
5. 恢复 WaitGraph 边
6. 恢复 FairScheduler 队列
7. 处理中断的 bus_send_and_wait
8. 如有 pending_task，重新派发
9. 发送系统消息通知恢复完成
```

### 5.2 上下文注入

恢复后的 agent 是全新进程，没有之前的对话历史。通过在 `session/new` 的 system prompt 中追加恢复上下文：

```
[Session recovered] You were previously working in this channel. Here is a summary of recent context:

- [alice → bob]: 请分析一下 scheduler 的性能瓶颈
- [bob → alice]: 主要问题在 VecDeque 的线性扫描...
- [system]: Agent bob completed task

You were working on: {current_task}
Resume your work from where you left off.
```

**注意**：注入的是摘要而非完整历史，避免超出 context window。

### 5.3 WaitGraph 恢复

```rust
impl WaitGraph {
    pub fn restore(&mut self, edges: &HashMap<String, String>) {
        self.edges = edges.clone();
    }
}
```

恢复时需要验证边的有效性：
- 如果等待目标 agent 也已崩溃且未恢复 → 移除该边，超时等待方
- 如果等待目标 agent 已正常在线 → 保留边，重新计时

### 5.4 中断的 send_and_wait 处理

三种情况：

| 场景 | 处理方式 |
|------|---------|
| A 在等待 B 的回复时崩溃 | A 恢复后，重新发送等待请求给 B，超时从剩余时间开始计算 |
| B 在处理 A 的请求时崩溃 | B 恢复后，从 checkpoint 中恢复 pending task，继续处理 |
| A 和 B 都崩溃 | 按依赖顺序恢复：先恢复 B（被等待方），再恢复 A |

对于超时已过的等待：直接以 timeout 错误完成，不再重试。

## 6. 失败场景与降级策略

### 6.1 Checkpoint 损坏或缺失

**策略**：降级为冷启动
- 重新 spawn agent（无上下文注入）
- 从 Snapshot 恢复消息历史（如有）
- 丢弃队列和等待状态
- 通知用户状态已部分丢失

### 6.2 进程无法重启

**策略**：标记 agent 为 Error 状态
- 可能原因：adapter 命令不存在、环境变量缺失、端口冲突
- 清理该 agent 的 WaitGraph 边
- 超时所有等待该 agent 的 send_and_wait
- 通知用户手动排查

### 6.3 握手失败

**策略**：指数退避重试
- 第 1 次：立即重试
- 第 2 次：等 5 秒
- 第 3 次：等 15 秒
- 超过 3 次：放弃，标记为 Error

### 6.4 上下文注入后 agent 行为异常

**策略**：无法自动判断，依赖用户反馈
- 恢复后发送系统消息提示用户 agent 已恢复
- 用户可通过 `/cancel <name>` + 重新发消息来手动干预

## 7. 实现计划

### Phase 1: Checkpoint 基础设施
- 新建 `crates/acp-core/src/checkpoint.rs`
- 实现 `Checkpoint` 序列化/反序列化
- 在 `Channel` 中添加 checkpoint 触发点
- 原子写入保障

### Phase 2: 崩溃检测与自动重启
- 扩展 `ClientEvent::Exited` 处理
- 实现 `ExitType` 分类
- 添加 `RecoveryState` 计数器
- 自动 re-spawn 逻辑

### Phase 3: 状态恢复
- 上下文摘要注入
- WaitGraph 恢复与验证
- FairScheduler 队列恢复
- 中断的 send_and_wait 处理

### Phase 4: 集成测试
- 模拟 agent 崩溃场景
- 验证恢复后消息传递正确性
- 验证 WaitGraph 无误报
- 验证队列不丢消息

## 8. 对现有代码的影响

| 文件 | 改动类型 | 描述 |
|------|---------|------|
| `crates/acp-core/src/checkpoint.rs` | 新建 | Checkpoint 模型与序列化 |
| `crates/acp-core/src/channel.rs` | 修改 | 添加 checkpoint 触发、恢复入口 |
| `crates/acp-core/src/client.rs` | 修改 | 崩溃分类、自动重启逻辑 |
| `crates/acp-core/src/agent.rs` | 修改 | 添加 RecoveryState 字段 |
| `crates/acp-core/src/fair_scheduler.rs` | 修改 | 序列化/反序列化队列状态 |
| `crates/acp-core/src/store.rs` | 修改 | Checkpoint 读写方法 |
| `crates/acp-core/src/lib.rs` | 修改 | 导出 checkpoint 模块 |

## 9. 开放问题

1. **Session 复用**：Claude API 的 session 能否在进程重启后复用？如果不能，恢复只能靠上下文注入，会有信息损失。需要实验验证。

2. **Terminal 状态**：`TerminalManager` 管理的伪终端在进程死亡后变为孤儿。恢复时应清理旧终端并按需重建，但无法恢复终端中正在运行的命令。

3. **部分输出处理**：agent 崩溃时 `stream_buf` 中的部分输出是否应展示给用户？建议展示并标记为 `[incomplete]`。

4. **多 agent 同时崩溃**：如果宿主进程的 bus socket 出问题，可能导致多个 agent 同时崩溃。恢复顺序应按依赖关系（WaitGraph 拓扑排序）进行，被等待的 agent 先恢复。
