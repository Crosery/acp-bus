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

Rust workspace with 4 crates + binary. Dependency direction: `acp-protocol ← acp-core ← acp-server / acp-tui ← main`

- **acp-protocol** — JSON-RPC 2.0 message types, `LineCodec`, ACP handshake/session/reverse-request serde definitions. Pure types, no IO.
- **acp-core** — Core domain: `Channel` (message history + broadcast events), `Agent` (Local/Spawned with status tracking), `Router` (@mention parsing → route targets, max depth 5), `Scheduler` (serial main-agent queue, max 10), `AcpClient` (child process lifecycle + JSON-RPC request/response correlation), `Adapter` (predefined configs: claude/c1/c2/gemini/codex with env var remapping), `TerminalManager` (subprocess capture with 1MB byte limit), `Store` (JSON snapshots to `~/.local/share/acp-bus/channels/`), `Registry` (global channel map).
- **acp-server** — JSON-RPC server over stdio for Neovim. Handles reverse requests (fs/read_text_file, terminal/*).
- **acp-tui** — ratatui TUI. `App` runs a `tokio::select!` loop polling crossterm events + channel broadcast at 50ms intervals.

## Key Patterns

- **Concurrency**: `Arc<Mutex<T>>` for shared state, `tokio::sync::broadcast` for channel event fan-out, `tokio::sync::mpsc` for client communication, `tokio::select!` for event multiplexing.
- **Error handling**: `anyhow::Result` throughout; `acp_protocol::RpcError` at the JSON-RPC boundary.
- **Process management**: Child processes spawned with `kill_on_drop`. AcpClient runs separate tokio tasks for stdin writer, stdout reader, stderr logger.
- **ACP handshake sequence**: `initialize` → `authenticate` (optional) → `session/new` → `session/prompt`. Agent sends `session/update` notifications for streaming content.
- **Store paths**: `~/.local/share/acp-bus/channels/{encoded_cwd}/{channel_id}.json` where CWD is encoded (trim `/`, replace `/` with `-`).

## Testing Conventions

Unit tests are inline `#[cfg(test)]` modules. Key tested areas: router (mention parsing, depth limits), scheduler (queue logic), protocol (JSON-RPC encoding), adapters.
