# Repository Guidelines

## Project Structure & Module Organization

This repository is a Rust workspace centered on `acp-bus`. The root binary lives in `src/main.rs` and wires together the workspace crates under `crates/`:

- `crates/acp-protocol`: JSON-RPC and ACP protocol types
- `crates/acp-core`: routing, clients, adapters, storage, and shared domain logic
- `crates/acp-server`: stdio JSON-RPC server for editor integrations
- `crates/acp-tui`: terminal UI built with `ratatui`
- `crates/acp-mock`: local mock binary for testing and debugging
- `crates/acp-bus-mcp`: MCP bridge process used by agent sessions

Inline unit tests live next to implementation files. Integration tests are in `crates/acp-core/tests/`.

## Build, Test, and Development Commands

- `cargo build`: build the entire workspace
- `cargo test`: run all unit and integration tests
- `cargo test -p acp-core`: run tests for one crate
- `cargo test -p acp-core router`: run tests matching a name
- `cargo run -- tui`: start the terminal UI
- `cargo run -- serve --stdio`: run the JSON-RPC server over stdio
- `cargo run -- channels`: list saved channel snapshots

For verbose diagnostics, set `RUST_LOG=debug` before running commands.

## Coding Style & Naming Conventions

Use standard Rust style: 4-space indentation, `snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep crates focused by responsibility and prefer small, composable functions over large handlers.

Run `cargo fmt` before submitting changes. Use `cargo clippy --all-targets --all-features` when adding or reshaping logic.

## Testing Guidelines

Add unit tests alongside the code they exercise and integration tests under `crates/acp-core/tests/` for cross-module behavior. Name tests for the expected behavior, for example `routes_mentions_to_target_agent`.

Cover router behavior, protocol encoding, adapter setup, and client/session flows when touching those areas. Run targeted tests first, then `cargo test` for the full workspace.

## Commit & Pull Request Guidelines

This repository currently has no commit history, so use short imperative commit messages such as `Add MCP handshake retry logging` or `Fix TUI channel redraw`. Keep one logical change per commit when practical.

Pull requests should include a concise summary, affected crates, test results, and any manual verification steps. Include screenshots only for TUI changes.

## Security & Configuration Tips

Do not commit secrets, tokens, or machine-specific socket paths. Prefer environment variables for runtime configuration such as `RUST_LOG` and agent socket settings.
