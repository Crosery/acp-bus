# 群组调度与任务完成追踪修复方案

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复群组讨论只跑 1 轮就停止、PendingTasks 时序竞态导致过早通知 main 的两个 P0 问题

**Architecture:** 两个独立问题各自修复。问题 1（rounds 不传）：将 bus_create_group + bus_group_message 合并为一个原子操作 `bus_start_group_discussion`，由系统控制 rounds 而非依赖 LLM 传参。问题 2（PendingTasks 时序）：引入 `hold()` 机制，在群组调度期间冻结 complete() 触发，群组结束后解冻并一次性结算。

**Tech Stack:** Rust, tokio, serde_json

**Dependencies:** Task 1 → Task 2 → Task 3（串行）。Task 2 依赖 Task 1 的 hold/release API。Task 4、5 可在 Task 3 后并行。

---

## 问题根因

### 问题 1：Main agent 不传 rounds 参数

**日志证据**: 080438、081830 — 辩论只跑 1 轮

LLM agent 在 content 文本中写了"共3轮"，但工具调用时没传 `rounds=3`，默认 1。tool description 改了两次仍然不够——LLM 不可靠地传递可选参数。

**根本解法**: 不依赖 LLM 传 rounds。提供一个高层 tool `bus_start_group_discussion`，参数包含 `rounds`（required），内部自动创建群组并发送消息。

### 问题 2：PendingTasks 时序竞态

**日志证据**: 074220 — 第 1 轮结束 main 就说"辩论结束"

时序：
1. `bus_create_agent(agent, task="你是辩手")` → `pt.track(agent)`
2. agent 的初始 task（角色描述）完成 → `pt.complete(agent)` → **all_done=true → 通知 main**
3. `bus_group_message` 还没调用，但 main 已经收到"全部完成"

`do_prompt_inner` 中 `group.is_none()` 的 guard 只能防止群组 prompt 内部的误触发，无法防止初始 task 完成时的误触发。

**根本解法**: PendingTasks 增加 `hold(tag)` / `release(tag)` 机制。群组调度开始时 hold，期间所有 complete() 正常移除 agent 但不触发 all_done 通知；群组调度结束后 release 并检查是否 all_done。

---

## 文件结构

### 修改文件
| 文件 | 改动 |
|------|------|
| `crates/acp-core/src/pending_tasks.rs` | 增加 hold/release 机制 |
| `crates/acp-core/src/bus_types.rs` | 新增 `BusEvent::StartGroupDiscussion` variant |
| `crates/acp-core/src/bus_socket.rs` | 新增 `start_group_discussion` 消息处理 |
| `crates/acp-bus-mcp/src/main.rs` | 新增 `bus_start_group_discussion` tool 定义 |
| `crates/acp-tui/src/app/bus_events.rs` | 新增 `StartGroupDiscussion` handler |
| `crates/acp-tui/src/app/prompting.rs` | 移除 do_prompt_inner 中的 pt.complete() 对群组的特殊处理，改用 hold/release |

---

## Task 1: PendingTasks hold/release 机制

**Files:**
- Modify: `crates/acp-core/src/pending_tasks.rs`

### 设计

```rust
pub struct PendingTasks {
    agents: HashSet<String>,
    ever_tracked: bool,
    holds: u32,  // >0 时 complete() 不触发 all_done
}

impl PendingTasks {
    /// 增加一个 hold。期间 complete() 正常移除 agent 但返回 false。
    pub fn hold(&mut self) { self.holds += 1; }

    /// 释放一个 hold。如果 holds 归零且 all_done，返回 true。
    pub fn release(&mut self) -> bool {
        self.holds = self.holds.saturating_sub(1);
        self.holds == 0 && self.ever_tracked && self.agents.is_empty()
    }

    /// complete() 在 holds > 0 时永远返回 false。
    pub fn complete(&mut self, agent_name: &str) -> bool {
        self.agents.remove(agent_name);
        self.holds == 0 && self.ever_tracked && self.agents.is_empty()
    }
}
```

- [ ] **Step 1: 写失败测试 — hold 期间 complete 不触发**

```rust
#[test]
fn hold_suppresses_all_done() {
    let mut pt = PendingTasks::new();
    pt.track("a");
    pt.track("b");
    pt.hold();
    assert!(!pt.complete("a")); // held
    assert!(!pt.complete("b")); // held, even though all agents removed
    assert!(pt.release());      // release → all_done
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test -p acp-core hold_suppresses`
Expected: FAIL — `hold` method not defined

- [ ] **Step 3: 写失败测试 — release 时仍有 pending agent**

```rust
#[test]
fn release_with_pending_agents_returns_false() {
    let mut pt = PendingTasks::new();
    pt.track("a");
    pt.track("b");
    pt.hold();
    assert!(!pt.complete("a"));
    assert!(!pt.release()); // b still pending
    assert!(pt.complete("b")); // now all done, no holds
}
```

- [ ] **Step 4: 写失败测试 — 多重 hold 需要多次 release**

```rust
#[test]
fn nested_holds() {
    let mut pt = PendingTasks::new();
    pt.track("a");
    pt.hold();
    pt.hold();
    assert!(!pt.complete("a"));
    assert!(!pt.release()); // still 1 hold
    assert!(pt.release());  // 0 holds, all done
}
```

- [ ] **Step 5: 写失败测试 — 模拟真实场景：initial task 在 hold 期间完成**

```rust
#[test]
fn initial_task_completes_during_group_hold() {
    let mut pt = PendingTasks::new();
    pt.track("debater-a");
    pt.track("debater-b");
    pt.track("researcher");

    pt.hold(); // group discussion about to start

    // researcher finishes independent task (non-group)
    assert!(!pt.complete("researcher")); // held

    // debater initial tasks finish (non-group)
    assert!(!pt.complete("debater-a")); // held
    assert!(!pt.complete("debater-b")); // held

    // group discussion runs... (no complete() calls during group prompts)

    // group discussion ends
    assert!(pt.release()); // all agents removed + 0 holds → true
}
```

- [ ] **Step 6: 跑测试确认全部失败**

Run: `cargo test -p acp-core pending_tasks`
Expected: 4 new tests FAIL

- [ ] **Step 7: 实现 hold/release**

在 `PendingTasks` struct 中增加 `holds: u32` 字段，修改 `new()`、`complete()`，增加 `hold()` 和 `release()` 方法。

- [ ] **Step 8: 跑测试确认全部通过**

Run: `cargo test -p acp-core pending_tasks`
Expected: 14 tests PASS (10 existing + 4 new)

- [ ] **Step 9: Commit**

```bash
git add crates/acp-core/src/pending_tasks.rs
git commit -m "feat: add hold/release to PendingTasks for group dispatch coordination"
```

---

## Task 2: 新增 bus_start_group_discussion 原子操作

**Files:**
- Modify: `crates/acp-core/src/bus_types.rs` — 新增 BusEvent variant
- Modify: `crates/acp-bus-mcp/src/main.rs` — 新增 MCP tool 定义
- Modify: `crates/acp-core/src/bus_socket.rs` — 新增 socket 消息处理
- Modify: `crates/acp-tui/src/app/bus_events.rs` — 新增 event handler

### 设计

新 tool `bus_start_group_discussion`:
```json
{
  "name": "bus_start_group_discussion",
  "description": "Create a group and start a multi-round discussion. Combines group creation + message dispatch in one atomic call. Use this for debates, reviews, or any multi-agent discussion.",
  "inputSchema": {
    "properties": {
      "group": { "type": "string", "description": "Group name" },
      "members": { "type": "array", "items": { "type": "string" }, "description": "Agent names to include" },
      "topic": { "type": "string", "description": "Discussion topic/instructions for the group" },
      "rounds": { "type": "integer", "description": "Number of discussion rounds (1-10)" }
    },
    "required": ["group", "members", "topic", "rounds"]
  }
}
```

`rounds` 是 **required** — LLM 必须传。

新 BusEvent variant:
```rust
StartGroupDiscussion {
    from_agent: String,
    group_name: String,
    members: Vec<String>,
    topic: String,
    rounds: u32,
    reply_tx: oneshot::Sender<BusSendResult>,
}
```

Handler 逻辑（bus_events.rs）：
1. **`pt.hold()` — 第一步就冻结 PendingTasks**（防止竞态：agent 初始 task 可能在此期间完成）
2. 创建群组（如果不存在）
3. 添加成员
4. post group message
5. `tokio::spawn(dispatch_group_sequential(...))`
6. 在 spawn 结束时 `pt.release()` 并检查 all_done（panic 安全：用 scopeguard 确保 release）
7. reply_tx 立即返回

- [ ] **Step 1: 在 bus_types.rs 添加 StartGroupDiscussion variant**

```rust
StartGroupDiscussion {
    from_agent: String,
    group_name: String,
    members: Vec<String>,
    topic: String,
    rounds: u32,
    reply_tx: oneshot::Sender<BusSendResult>,
},
```

- [ ] **Step 2: 在 acp-bus-mcp/src/main.rs tools/list 添加 tool 定义**

在 `bus_group_message` 后面添加 `bus_start_group_discussion` 的 JSON schema（rounds 为 required）。

- [ ] **Step 3: 在 acp-bus-mcp/src/main.rs tools/call 添加 tool 处理**

```rust
"bus_start_group_discussion" => {
    let group = args.get("group").and_then(|v| v.as_str()).unwrap_or("");
    let members: Vec<String> = args.get("members")
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let topic = args.get("topic").and_then(|v| v.as_str()).unwrap_or("");
    let rounds = args.get("rounds").and_then(|v| v.as_u64()).unwrap_or(3);
    json!({
        "type": "start_group_discussion",
        "from": agent_name,
        "group": group,
        "members": members,
        "topic": topic,
        "rounds": rounds
    })
}
```

- [ ] **Step 4: 在 bus_socket.rs 添加 socket 消息处理**

在 `"group_message"` 分支后添加 `"start_group_discussion"` 分支，解析 group/members/topic/rounds，发送 `BusEvent::StartGroupDiscussion`。

- [ ] **Step 5: 在 bus_events.rs 添加 event handler**

```rust
BusEvent::StartGroupDiscussion {
    from_agent, group_name, members, topic, rounds, reply_tx,
} => {
    // 1. Hold PendingTasks FIRST — prevents race with agent initial tasks completing
    {
        let mut pt = ctx.pending_tasks.lock().await;
        pt.hold();
    }

    // 2. Create group
    let mut ch = ctx.channel.lock().await;
    if ch.create_group(&group_name, &from_agent) {
        for member in &members {
            if let Some(group) = ch.groups.get_mut(&group_name) {
                group.add_member(member);
            }
        }
    }
    // 3. Post initial message
    ch.post_group(&group_name, &from_agent, &topic);
    if let Some(agent) = ch.agents.get_mut(&from_agent) {
        agent.has_bus_activity = true;
    }
    let history = ch.group_history(&group_name, 10);
    let recipients = ch.group_recipients(&group_name, &from_agent);
    drop(ch);

    // 4. Spawn group dispatch (with panic-safe release)
    let ctx2 = ctx.clone();
    let pt_ref = ctx.pending_tasks.clone();
    tokio::spawn(async move {
        // Ensure release() is called even if dispatch panics
        let _guard = scopeguard::guard(pt_ref.clone(), |pt| {
            // Can't .await in Drop, so use try_lock as best-effort
            if let Ok(mut pt) = pt.try_lock() {
                pt.release();
            }
        });

        dispatch_group_sequential(
            recipients, history, group_name.clone(), from_agent, topic, ctx2.clone(), rounds,
        ).await;

        // Normal path: release hold and check all_done
        // Defuse the guard since we're releasing manually
        let pt = scopeguard::ScopeGuard::into_inner(_guard);
        let all_done = {
            let mut pt = pt.lock().await;
            pt.release()
        };
        if all_done {
            tokio::spawn(do_prompt(
                "main".to_string(),
                format!("[System: Group '{group_name}' discussion completed ({rounds} round(s)). Review the group history and deliver a summary to the user.]"),
                ctx2,
            ));
        }
    });

    let _ = reply_tx.send(BusSendResult {
        message_id: None, delivered: true, error: None,
    });
}
```

**注意**：需要在 `crates/acp-tui/Cargo.toml` 添加 `scopeguard = "1"` 依赖。如果不想引入依赖，也可以用一个简单的自定义 drop guard struct。

- [ ] **Step 6: cargo build 确认编译通过**

Run: `cargo build`

- [ ] **Step 7: cargo test --workspace 确认无回归**

Run: `cargo test --workspace`

- [ ] **Step 8: Commit**

```bash
git add crates/acp-core/src/bus_types.rs crates/acp-bus-mcp/src/main.rs \
       crates/acp-core/src/bus_socket.rs crates/acp-tui/src/app/bus_events.rs
git commit -m "feat: add bus_start_group_discussion atomic tool"
```

---

## Task 3: 清理旧的 GroupMessage 中的 dispatch_group_sequential completion 逻辑

**Files:**
- Modify: `crates/acp-tui/src/app/prompting.rs` — dispatch_group_sequential 末尾的 pt.complete() 逻辑移除（已由 StartGroupDiscussion handler 的 hold/release 替代）
- Modify: `crates/acp-tui/src/app/bus_events.rs` — GroupMessage handler 也加 hold/release（向后兼容）

**依赖:** Task 1 + Task 2

- [ ] **Step 1: 从 dispatch_group_sequential 末尾移除 pt.complete() 块**

移除 prompting.rs 中 `dispatch_group_sequential` 函数末尾的 "All rounds complete — mark group members as done" 代码块（约 15 行）。这个职责现在由调用方（StartGroupDiscussion handler 和 GroupMessage handler）通过 hold/release 承担。

- [ ] **Step 2: GroupMessage handler 也加 hold/release + all_done 通知**

在 bus_events.rs 的 `BusEvent::GroupMessage` 分支中：
1. spawn 前 `pt.hold()`
2. spawn 内部用 scopeguard 确保 panic 安全
3. dispatch 结束后 `pt.release()`，若 all_done=true 则 spawn do_prompt 通知 main

```rust
// 在 tokio::spawn 前:
{
    let mut pt = ctx.pending_tasks.lock().await;
    pt.hold();
}

// spawn 内部（dispatch 后）:
let all_done = {
    let mut pt = ctx2.pending_tasks.lock().await;
    pt.release()
};
if all_done {
    tokio::spawn(do_prompt(
        "main".to_string(),
        format!("[System: Group '{gn}' discussion completed ({rounds} round(s), {n} participants). Review the group history and deliver a summary to the user.]",
            rounds = rounds, n = recipients.len()),
        ctx2,
    ));
}
```

- [ ] **Step 3: cargo build + cargo test --workspace**

- [ ] **Step 4: Commit**

```bash
git add crates/acp-tui/src/app/prompting.rs crates/acp-tui/src/app/bus_events.rs
git commit -m "refactor: move group completion tracking to hold/release in event handlers"
```

---

## Task 4: 更新 main agent 提示词引导使用新 tool

**Files:**
- Modify: `crates/acp-core/src/adapter.rs`

- [ ] **Step 1: 在 main agent prompt 的 "When to Create Groups" 段落后添加引导**

在现有的 "When to Create Groups" 段落末尾追加：

```
## Starting Group Discussions

When you need agents to discuss, debate, or review together:
1. First create the agents via bus_create_agent with their role descriptions
2. Then use bus_start_group_discussion to create the group and start the discussion in ONE call
3. Set rounds based on the discussion complexity: 1 for quick opinions, 3 for debates, 5 for deep reviews

Do NOT use bus_create_group + bus_group_message separately — use bus_start_group_discussion instead.
```

- [ ] **Step 2: cargo build 确认编译**

- [ ] **Step 3: Commit**

```bash
git add crates/acp-core/src/adapter.rs
git commit -m "feat: guide main agent to use bus_start_group_discussion"
```

---

## Task 5: 更新测试场景文档

**Files:**
- Modify: `docs/test-scenarios.md`

- [ ] **Step 1: 更新场景 1 和场景 5 的预期行为**

场景 1（辩论）和场景 5（单轮群组）应注明 main 现在会使用 `bus_start_group_discussion` 而非 `bus_create_group` + `bus_group_message`。

- [ ] **Step 2: 新增场景 6 验证 hold/release**

```markdown
## 场景 6：初始任务 + 群组讨论混合（PendingTasks 时序验证）

验证：agent 初始任务完成不会在群组讨论开始前触发 all-done。

\```
创建3个agent（正方、反方、裁判），每个给一个简短的角色描述任务。
然后立即用 bus_start_group_discussion 开一场 2 轮辩论。
\```

**预期**：
- [ ] 3 个 agent 的初始任务完成时 main 不收到 all-done 通知
- [ ] 辩论跑满 2 轮
- [ ] 辩论结束后 main 才收到通知

**失败标志**：初始任务完成后 main 就说"全部完成"
```

- [ ] **Step 3: Commit**

```bash
git add docs/test-scenarios.md
git commit -m "docs: update test scenarios for bus_start_group_discussion"
```
