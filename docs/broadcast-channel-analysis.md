# Tokio Broadcast Channel 架构分析

## 概述

acp-bus 使用 `tokio::sync::broadcast` 作为核心事件分发机制，实现 Channel 到多个订阅者（TUI + Agents）的 1:N 事件扇出。

## 实现细节

- **代码位置**：`crates/acp-core/src/channel.rs:102`，`broadcast::channel(256)`
- **事件类型**：`NewMessage`、`AgentStatus`、`Closed` — 体积小，Clone 开销低
- **消费端**：TUI 的 `tokio::select!` 循环 + 各 AcpClient 子进程

## 选型对比

| 方案 | 模式 | 优势 | 劣势 |
|------|------|------|------|
| **broadcast**（当前） | 1:N 扇出 | 每个 subscriber 独立 Receiver，互不干扰；非阻塞发送；代码简洁 | 慢消费者丢消息（Lagged）；每个 subscriber clone 完整 Message |
| mpsc | N:1 | 天然背压 | 需手动维护 `Vec<Sender>` 做扇出，复杂度高 |
| watch | 1:N（最新值） | 零丢失最新状态 | 只保留最新值，丢失中间事件，不适合消息流 |

**结论**：broadcast 最匹配 acp-bus 的事件分发场景。

## 风险与改进建议

| 风险 | 现状 | 建议 |
|------|------|------|
| 慢消费者丢消息 | buffer=256，未处理 `RecvError::Lagged` | 捕获 Lagged 后从 `messages Vec` 做历史重放补偿 |
| 无订阅者静默丢弃 | `let _ = send()` 忽略错误 | 当前可接受，按需加日志 |
| Clone 内存开销 | 事件体积小，开销可忽略 | 规模增长后可考虑 `Arc<Message>` 减少拷贝 |

## 架构亮点

### 单写多读零协调

Channel 使用 `&mut self` 保证唯一 Sender，天然避免多写者竞争。broadcast 在此单写多读模式下几乎零协调开销，无需额外加锁或同步。

### broadcast + mpsc 职责分离

- **broadcast** — 事件扇出通知（Channel → TUI + Agents），不承担可靠投递压力
- **mpsc** — AcpClient 内部请求-响应关联，保证一对一的消息送达

各司其职，每层只做一件事，简洁且可靠。

## 结论

当前规模（几个 agent + 1 TUI）下，broadcast channel 是最简洁合适的选择。

**短期唯一建议**：在消费端增加 `RecvError::Lagged` 处理——至少日志告警，理想情况下从 `channel.messages` Vec 做历史补偿重放。其余优化在当前规模下不必要。
