# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Test

```bash
cargo build                        # build all crates
cargo test                         # run all tests
cargo test -p acp-core             # test a single crate
cargo test -p acp-core router      # run tests matching "router" in acp-core
cargo run -- tui                   # TUI mode
cargo run -- serve --stdio         # JSON-RPC server mode (Neovim integration)
cargo run -- channels              # list saved channel snapshots
```

Tracing: set `RUST_LOG=debug` (or trace/info) — logs go to stderr.

Rust toolchain: stable (see `rust-toolchain.toml`).

## Architecture

Rust workspace with 5 crates + binary. Dependency direction: `acp-protocol ← acp-core ← acp-server / acp-tui ← main`

- **acp-protocol** — JSON-RPC 2.0 message types, `LineCodec`, ACP handshake/session/reverse-request serde definitions. Pure types, no IO.
- **acp-core** — Core domain:
  - `Channel` — message history + broadcast events + group registry
  - `Agent` — Local/Spawned with status tracking, `clean_tool_name()` strips MCP prefixes
  - `Router` — @mention parsing → route targets, max depth 5
  - `FairScheduler` — main-agent queue with priority (High/Normal) + fair round-robin rotation, max 15
  - `Group` — agent groups for multi-party discussion (name, creator, members)
  - `AcpClient` — child process lifecycle + JSON-RPC request/response correlation
  - `Adapter` — predefined configs (claude/c1/c2/gemini/codex), system prompts in English with `Always respond in Chinese`
  - `WaitGraph` — directed graph for send_and_wait deadlock detection
  - `TerminalManager`, `Store`, `Registry`, `CommLog`
- **acp-bus-mcp** — MCP server providing bus tools to agents: `bus_send_message`, `bus_send_and_wait`, `bus_reply`, `bus_list_agents`, `bus_create_agent`, `bus_remove_agent`, `bus_create_group`, `bus_group_message`
- **acp-server** — JSON-RPC server over stdio for Neovim. Handles reverse requests (fs/read_text_file, terminal/*).
- **acp-bus-mcp** tools also include: `bus_group_add` (add member to existing group)
- **acp-tui** — ratatui 0.29 TUI with markdown rendering (tui-markdown + custom header/code-block/list handling):
  - `app/mod.rs` — App struct, event loop (`tokio::select!`), UI rendering, key handling, delegates clipboard to clipboard.rs (~817 lines)
  - `app/bus_events.rs` — all BusEvent handlers (SendMessage, Reply, GroupMessage, etc.) (~481 lines)
  - `app/commands.rs` — slash command processing (/add, /remove, /group, etc.) (~284 lines)
  - `app/prompting.rs` — core prompt dispatch: `do_prompt_inner`, group sequential dispatch, agent command execution, elastic main instances (~860 lines)
  - `app/lifecycle.rs` — agent spawning, client handshake, event listener (~289 lines)
  - `app/clipboard.rs` — clipboard image reading (arboard + wl-paste fallback) (~106 lines)
  - `app/image.rs` — image temp file save/cleanup (~115 lines)
  - `components/messages.rs` — markdown-rendered messages, DM/group filtering, streaming preview
  - `components/status_bar.rs` — sidebar with 私聊/群组 tabs, agent status labels
  - `components/input.rs` — input box with Tab completion, agent status indicator
  - `layout.rs` — collapsible sidebar layout
  - `theme.rs` — color constants
  - `i18n.rs` — centralized English UI strings, all user-facing text constants (~518 lines)

## Module Rules (for AI and human maintainers)

- **No file over 800 lines** — if a module grows past 800 lines, split it by domain (bus events, commands, prompting, lifecycle, etc.)
- **app/mod.rs is the orchestrator** — it holds App struct, event loop, rendering, and delegates to submodules. Clipboard handling delegated to clipboard.rs. Don't add business logic here.
- **app/bus_events.rs** — all BusEvent match arms go here. New bus events = new match arm here.
- **app/commands.rs** — all `/command` handlers. New slash commands = new match arm here.
- **app/prompting.rs** — prompt construction, dispatch, routing, completion handling, elastic main instances. The core "brain" of agent communication.
- **app/lifecycle.rs** — agent process spawning, connection, event listening. Changes to how agents start/stop go here.
- **app/clipboard.rs** — clipboard image reading (arboard + wl-paste fallback). Produces `PendingImage` for prompting.
- **app/image.rs** — image temp file save/cleanup. Converts `PendingImage` to on-disk file for agent consumption.
- **i18n.rs** — centralized English UI strings. All user-facing text constants live here; future locale system only needs to swap this module.
- **components/** — pure UI rendering. No business logic, no async, no channel locks.
- **Visibility**: use `pub(crate)` for cross-module sharing within the crate. Only `pub` what's needed by other crates.
- **Group messages**: posted via `Channel::post_group()` which sets `group` field on the Message before emitting the event. Group prompts include conversation history via `format_group_prompt()` and are dispatched sequentially via `dispatch_group_sequential()`.
- **DM vs Group separation**: Messages with `group: Some(...)` only appear in group tabs; agent DM tabs exclude them.
- **Devlog**: 每次会话结束前，在 `docs/devlog/YYYY-MM-DD.md` 记录当日改动日志（问题→根因→修复→文件），同一天多次会话追加到同一文件。格式参考 `docs/devlog/2026-03-27.md`。

## Key Patterns

- **Concurrency**: `Arc<Mutex<T>>` for shared state, `tokio::sync::broadcast` for channel event fan-out, `tokio::sync::mpsc` for client communication, `tokio::select!` for event multiplexing.
- **Error handling**: `anyhow::Result` throughout; `acp_protocol::RpcError` at the JSON-RPC boundary.
- **Process management**: Child processes spawned in own process group (`process_group(0)`) with `kill_on_drop`. `force_kill()` sends `SIGKILL` to the entire process group.
- **ACP handshake sequence**: `initialize` → `authenticate` (optional) → `session/new` → `session/prompt`. Agent sends `session/update` notifications for streaming content.
- **Store paths**: `~/.local/share/acp-bus/channels/{encoded_cwd}/{channel_id}.json`.
- **Message sender context**: All agent-to-agent messages include sender prefix (`[Message from X]`, `[Async message from X]`, `[X is waiting for your reply (timeout Ns)]`, etc.) in English for better LLM instruction following.
- **Tool name display**: `Agent::clean_tool_name()` strips `mcp__<server>__` prefix for UI display (e.g., `mcp__acp-bus__bus_reply` → `bus_reply`).
- **Elastic main instances**: When main agent is busy, up to `MAX_MAIN_INSTANCES = 5` main instances are auto-spawned on demand (main-2, main-3, etc.) to handle concurrent user prompts.
- **Image paste flow**: `Ctrl+V` → `clipboard.rs` reads image → `image.rs` saves temp file → `prompting.rs` includes image in prompt.

## Agent System Prompts

System prompts are in **English** (better instruction following, lower token usage) with `Always respond in Chinese` directive. Located in `adapter.rs::get_bus_system_prompt()`.

Key prompt rules:
- Main agent: focus on WHAT to do, never teach agents HOW to use tools
- Worker agents: reply to sync waits immediately, don't chain sync calls
- Both: NEVER mention tool names in text output, just call them silently

## Bus Communication

- `bus_send_message` — async one-way, target gets `[Async message from X]` prefix
- `bus_send_and_wait` — sync with timeout (default 300s, max 600s), target gets `[X is waiting for your reply (timeout Ns)]`
- `bus_reply` — fulfills pending send_and_wait
- `bus_create_group` / `bus_group_message` — group multi-party messaging
- Deadlock detection via `WaitGraph` (directed cycle detection)
- When agent completes, all agents' `waiting_reply_from` pointing to it are cleared
- Elastic main: when main is busy, up to 5 main instances are spawned elastically to handle concurrent prompts

## System Message Types

`SystemKind` enum with distinct icons and colors:
- `AgentOnline` (▲ green), `AgentOffline` (▼ gray), `AgentComplete` (✓ green)
- `AgentError` (✗ red), `QueueNotice` (⏳ yellow), `Routing` (→ dim blue), `General` (· gray)

## TUI Keybindings

- `Ctrl+C` — quit (force-kill all agents)
- `Ctrl+B` — toggle sidebar collapse
- `Ctrl+Q` — cancel selected agent's prompt (all agents if on System tab)
- `Ctrl+V` — paste image from clipboard
- `Ctrl+Enter` — newline in input
- `Tab` — switch DM/Groups tabs in sidebar
- `Ctrl+N/P` — switch agent tabs or navigate completion popup
- `Shift+Arrow` — switch agent tabs
- `Enter` — send message or confirm completion selection
- `Ctrl+J/K` — scroll messages
- `Ctrl+D/U` — page scroll (10 lines)
- Mouse scroll — scroll messages

## TUI Commands

- `/add <name> <adapter> [task]` — create agent
- `/remove <name>` — remove agent
- `/group <subcommand>` — group management:
  - `/group create <name> <members...>` — create group
  - `/group add <name> <member>` — add member to group
  - `/group list` — list groups
  - `/group remove <name> <member>` — remove member
- `/list`, `/adapters`, `/cancel <name>`, `/save`, `/help`, `/quit`

## Markdown Rendering

Messages rendered via custom `render_markdown()` in `messages.rs`:
- Headers (`#`~`######`) — manually parsed, styled with colors
- Code blocks (` ``` `) — fence hidden, content indented with code style
- Lists (`- ` / `* `) — converted to `  • ` bullets
- Links (`[text](url)`) — URL hidden, text shown
- `---` — horizontal rule rendered as `────────`
- Bold/italic/inline-code — delegated to `tui-markdown` crate
- Streaming preview also rendered with markdown in real-time

## Testing Conventions

Unit tests are inline `#[cfg(test)]` modules. ~119 tests across workspace. Key tested areas:
- `acp-core`: router, scheduler (old), fair_scheduler, wait_graph, channel (incl. groups, SystemKind), agent (clean_tool_name, reset_stream), adapters, group model
- `acp-tui`: layout (sidebar collapse), input (status_line, placeholder), messages (markdown rendering: headers, bold, lists, links, code blocks, h4, hr)
- `acp-core` integration: client lifecycle, bus_send_message
- `acp-protocol`: JSON-RPC encoding, handshake params

## Implementation Plan

Full upgrade plan at `docs/superpowers/plans/2026-03-26-comprehensive-upgrade.md`.

Completed tasks: Task 1-8 (sidebar, input, system messages, streaming, groups, fair scheduler, markdown, prompts).

Remaining: Task 9-11 (encrypted prompt storage with AES-256-GCM + SHA-256 key derivation).
