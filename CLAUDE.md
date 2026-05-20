# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`shaft` — Secure Host Async File Transfer. TUI dual-pane file manager (like Midnight Commander) that transfers files over SFTP. Built with ratatui + crossterm + ssh2.

## Commands

```bash
cargo build                        # debug build
cargo build --release              # release build
cargo run                          # launch TUI, prompts for SSH connection
cargo run -- user@host             # launch and connect to remote immediately
cargo run -- user@host:2222        # custom port
cargo clippy                       # lint
cargo test                         # run tests (minimal right now)
```

No config files — SSH connection params come from `~/.ssh/config` via `ssh2-config`.

## Architecture

Two-pane layout: `left` (always local) and `right` (local or remote). Each pane is a `Pane` struct backed by a `Box<dyn FilesystemProvider>`.

**Key types:**

- `FilesystemProvider` (`src/fs/mod.rs`) — trait with `list_dir`, `read_file`, `write_file`, `delete`, `mkdir`, `rename`. Two impls: `LocalProvider` and `RemoteProvider` (SFTP).
- `Pane` (`src/pane.rs`) — cursor/scroll/selection state + display entries. Calls provider on navigation.
- `App` (`src/app.rs`) — owns both panes + both providers. `AppMode` is `Normal | Transfer(SharedProgress) | Dialog(DialogKind)`.
- `SshTarget` / `SshConnection` (`src/ssh.rs`) — parses `user@host[:port]`, applies `~/.ssh/config`, tries pubkey → agent → password auth in that order.
- `transfer.rs` — `copy_entry` / `move_entry`. Transfers are **synchronous** (ssh2 SFTP constraint); progress updates happen post-transfer. The `SharedProgress` `Arc<Mutex<TransferProgress>>` exists for future async upgrade.

**Event flow:** `main.rs` polls crossterm events every 50ms → `App::handle_event` → `App::handle_key` → maps to `Action` via `keybinds::map_key` → mutates pane/provider state.

**Keybinds** (`src/keybinds.rs`):
`j/k/↑/↓` navigate, `Tab` switch pane, `Enter` open dir, `h/Bsp` go up, `~` home, `Space` select, `F5` copy, `F6` move, `F7` mkdir, `F8/Del` delete, `q` quit.

**Icons** use Nerd Font codepoints — terminal must have a Nerd Font installed.

## Transfer limitation

`copy_entry` reads the entire file into memory before writing (ssh2 SFTP write-chunking requires an open file handle). Progress bar only updates after the write completes. Large-file support needs a streaming rewrite using `ssh2::Sftp::open` / `File` handles.
