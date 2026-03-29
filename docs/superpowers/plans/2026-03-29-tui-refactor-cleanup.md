# TUI Refactor: Chinese→English Prompts, Module Split, Doc Update

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Convert all Chinese UI/prompt strings to English, split oversized modules into focused files, update CLAUDE.md to reflect current architecture.

**Architecture:** Pure refactoring — no behavior changes. Extract clipboard logic from mod.rs into `app/clipboard.rs`, extract image handling from prompting.rs into `app/image.rs`. Create a centralized `i18n.rs` for all UI strings. Update CLAUDE.md.

**Tech Stack:** Rust, ratatui, existing crate structure

**Baseline:** 148 tests pass, 0 fail. `cargo build` clean.

---

### Task 1: Create centralized UI strings module (`i18n.rs`)

**Files:**
- Create: `crates/acp-tui/src/i18n.rs`
- Modify: `crates/acp-tui/src/lib.rs`

- [ ] **Step 1: Write failing test**

```rust
// In crates/acp-tui/src/i18n.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_labels_not_empty() {
        assert!(!STATUS_IDLE.is_empty());
        assert!(!STATUS_THINKING.is_empty());
        assert!(!STATUS_STREAMING.is_empty());
        assert!(!STATUS_ERROR.is_empty());
        assert!(!STATUS_CONNECTING.is_empty());
        assert!(!STATUS_DISCONNECTED.is_empty());
    }

    #[test]
    fn system_messages_not_empty() {
        assert!(!SYS_AGENT_ONLINE.is_empty());
        assert!(!SYS_AGENT_COMPLETE.is_empty());
        assert!(!SYS_QUEUE_FULL.is_empty());
        assert!(!SYS_NO_CLIPBOARD_IMAGE.is_empty());
    }
}
```

- [ ] **Step 2: Run test — expect FAIL (module not found)**

Run: `cargo test -p acp-tui i18n -- -v`

- [ ] **Step 3: Implement i18n.rs with all UI strings in English**

All Chinese strings from these files get English constants here:
- `status_bar.rs`: status labels (空闲→Idle, 思考中→Thinking, etc.), sidebar hints, group labels
- `input.rs`: placeholder text, command descriptions, status labels
- `lifecycle.rs`: connection/online/exit messages
- `prompting.rs`: queue notices, timeout messages, completion messages
- `bus_events.rs`: group error messages
- `commands.rs`: help text, error messages
- `mod.rs`: clipboard messages, cancel messages

- [ ] **Step 4: Add `pub mod i18n;` to lib.rs**

- [ ] **Step 5: Run test — expect PASS**

Run: `cargo test -p acp-tui i18n -- -v`

- [ ] **Step 6: Commit**

```bash
git add crates/acp-tui/src/i18n.rs crates/acp-tui/src/lib.rs
git commit -m "feat(tui): add centralized i18n module with English UI strings"
```

---

### Task 2: Replace Chinese strings with i18n constants across TUI

**Files:**
- Modify: `crates/acp-tui/src/components/status_bar.rs` (~19 occurrences)
- Modify: `crates/acp-tui/src/components/input.rs` (~13 occurrences)
- Modify: `crates/acp-tui/src/app/lifecycle.rs` (~5 occurrences)
- Modify: `crates/acp-tui/src/app/prompting.rs` (~6 occurrences)
- Modify: `crates/acp-tui/src/app/bus_events.rs` (~2 occurrences)
- Modify: `crates/acp-tui/src/app/commands.rs` (~7 occurrences)
- Modify: `crates/acp-tui/src/app/mod.rs` (~1 occurrence)
- Modify: `crates/acp-tui/src/components/messages.rs` (~2 occurrences in tests)

- [ ] **Step 1: Replace all Chinese strings in status_bar.rs**

Key replacements: `"空闲"` → `i18n::STATUS_IDLE`, `"思考中"` → `i18n::STATUS_THINKING`, etc.
Update test assertions to match new English strings.

- [ ] **Step 2: Replace all Chinese strings in input.rs**

Key replacements: placeholder text, command descriptions, status labels.
Update test assertions.

- [ ] **Step 3: Replace all Chinese strings in lifecycle.rs, prompting.rs, bus_events.rs, commands.rs, mod.rs**

System messages: `"正在连接…"` → `i18n::SYS_CONNECTING`, etc.
Prompt strings (image paste) must be in English.

- [ ] **Step 4: Update test assertions in messages.rs**

Change `"已上线"` → English equivalent in test assertions.

- [ ] **Step 5: Run full test suite**

Run: `cargo test --workspace`
Expected: 148+ tests pass, 0 fail

- [ ] **Step 6: Commit**

```bash
git add -A crates/acp-tui/src/
git commit -m "refactor(tui): replace all Chinese UI strings with i18n constants"
```

---

### Task 3: Extract clipboard logic into `app/clipboard.rs`

**Files:**
- Create: `crates/acp-tui/src/app/clipboard.rs`
- Modify: `crates/acp-tui/src/app/mod.rs` (remove ~80 lines)

- [ ] **Step 1: Write failing test**

```rust
// In crates/acp-tui/src/app/clipboard.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_images_starts_empty() {
        let pi = PendingImages::default();
        assert!(pi.images.is_empty());
    }
}
```

- [ ] **Step 2: Run test — expect FAIL**

Run: `cargo test -p acp-tui clipboard -- -v`

- [ ] **Step 3: Move `PendingImage`, `PendingImages`, `read_clipboard_image()`, `try_paste_image()` from mod.rs to clipboard.rs**

`mod.rs` keeps a thin `try_paste_image` method on App that delegates to `clipboard::read_clipboard_image()`.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`
Expected: 148+ pass

- [ ] **Step 5: Commit**

```bash
git add crates/acp-tui/src/app/clipboard.rs crates/acp-tui/src/app/mod.rs
git commit -m "refactor(tui): extract clipboard logic into app/clipboard.rs"
```

---

### Task 4: Extract image handling into `app/image.rs`

**Files:**
- Create: `crates/acp-tui/src/app/image.rs`
- Modify: `crates/acp-tui/src/app/prompting.rs` (remove ~30 lines)

- [ ] **Step 1: Write failing test**

```rust
// In crates/acp-tui/src/app/image.rs
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn save_and_cleanup_round_trip() {
        // Test that save_paste_image returns a path and cleanup removes it
        // (uses temp dir, not real cwd)
    }
}
```

- [ ] **Step 2: Run test — expect FAIL**

- [ ] **Step 3: Extract image save/cleanup logic from prompting.rs**

Move the `if let Some(img) = image { ... }` block and `paste_file` cleanup into `image::save_paste_image()` and `image::cleanup_paste_file()`.

- [ ] **Step 4: Run full test suite**

Run: `cargo test --workspace`

- [ ] **Step 5: Commit**

```bash
git add crates/acp-tui/src/app/image.rs crates/acp-tui/src/app/prompting.rs
git commit -m "refactor(tui): extract image save/cleanup into app/image.rs"
```

---

### Task 5: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update Architecture section**

Add new modules: `app/clipboard.rs`, `app/image.rs`, `i18n.rs`.
Update line counts and descriptions for all TUI modules.

- [ ] **Step 2: Update Module Rules**

Add clipboard and image module rules.
Update `app/mod.rs` description to note it no longer contains clipboard/image logic.

- [ ] **Step 3: Update Key Patterns**

Add elastic main instances pattern.
Add image paste flow.
Update keybinding table.

- [ ] **Step 4: Update TUI Keybindings section**

Add: `Ctrl+V` — paste image from clipboard
Update: `Tab` — switch DM/Groups (no longer completes)
Add: `Ctrl+Enter` — newline
Update Ctrl+N/P note about popup navigation.

- [ ] **Step 5: Update TUI Commands section**

Add `/group` command.
Update help text to English.

- [ ] **Step 6: Verify CLAUDE.md is consistent with actual code**

Grep for any claims that don't match code.

- [ ] **Step 7: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with new modules, keybindings, and architecture"
```

---

### Task 6: Code review + final verification

- [ ] **Step 1: Run full build**

Run: `cargo build`
Expected: exit 0, no warnings from our crates

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --workspace`
Expected: 0 errors, only pre-existing warnings

- [ ] **Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: all tests pass (148+)

- [ ] **Step 4: Verify no Chinese strings remain in code (excluding tests that validate Chinese output)**

Run: `grep -rn` for Chinese characters in `crates/acp-tui/src/` excluding test blocks

- [ ] **Step 5: Verify mod.rs line count is under 800**

- [ ] **Step 6: Final commit (if any fixups needed)**

---

## File Structure Summary

After refactoring:

```
crates/acp-tui/src/
├── lib.rs
├── i18n.rs              (NEW — all UI strings)
├── theme.rs
├── layout.rs
├── app/
│   ├── mod.rs           (~750 lines, down from 893)
│   ├── bus_events.rs
│   ├── commands.rs
│   ├── lifecycle.rs
│   ├── prompting.rs     (~840 lines, down from 877)
│   ├── clipboard.rs     (NEW — ~80 lines, clipboard read)
│   └── image.rs         (NEW — ~50 lines, image save/cleanup)
└── components/
    ├── input.rs
    ├── messages.rs
    └── status_bar.rs
```
