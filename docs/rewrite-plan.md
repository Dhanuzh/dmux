# dmux Rewrite Plan (tmux feature parity in Rust)

## What was analyzed

- `tmux/` (C implementation): monolithic codebase with strong command-centric structure.
- `zellij/` (Rust implementation): multi-crate client/server split with message-passing threads.

## Key tmux architecture facts

From `tmux/tmux.c`, `tmux/server.c`, `tmux/cmd.c`, and `tmux/tmux.h`:

- Client/server split already exists in tmux (CLI client talks to background server over Unix socket).
- Global state model is based on `session -> window -> pane` plus global/session/window options.
- Commands are first-class and table-driven; currently 90 command entries are wired in `cmd.c`.
- Command parsing is grammar-based (`cmd-parse.y`) and key bindings are central (`key-bindings.c`).
- Rendering and terminal behavior are built around `grid.*`, `input.*`, `tty.*`, `screen-write.c`, `status.c`.

## Key zellij architecture facts to reuse

From `zellij/src/main.rs`, `zellij/zellij-server/src/lib.rs`, and `zellij/zellij-client/src/lib.rs`:

- Clear crate split: CLI shim + client + server + shared utils/protocol.
- Server is a coordinator around channels and dedicated workers (screen, pty, plugin, background jobs).
- Typed instructions/events are used for inter-thread communication.
- Strong separation of OS I/O abstraction and business logic.

## dmux architecture recommendation

Use tmux semantics, zellij structure:

1. `dmux` (binary crate): CLI compatibility surface (`new-session`, `split-window`, `send-keys`, etc.).
2. `dmux-proto`: IPC request/response + event payloads.
3. `dmux-core`: canonical state model (`Session`, `Window`, `Pane`, options, key tables, formats).
4. `dmux-server`: socket accept loop + command dispatcher + PTY/layout/render orchestration.
5. `dmux-client`: transport and convenience API for CLI commands.

## Phased roadmap

1. Phase 0: Scaffold and transport
- Done in this repo: Rust workspace and Unix socket JSON protocol.

2. Phase 1: Core tmux object model
- Add IDs, links, active pointers, environment inheritance, and option scopes.
- Mirror tmux invariants for session/window/pane lifecycle.

3. Phase 2: Command parser/dispatcher
- Implement a tmux-compatible parser front-end.
- Start with high-frequency commands: `new-session`, `attach-session`, `split-window`, `send-keys`, `list-*`, `kill-*`.

4. Phase 3: PTY + terminal model
- Spawn PTYs per pane, support resize, input forwarding, lifecycle hooks.
- Build grid/scrollback representation compatible with copy-mode needs.

5. Phase 4: UI semantics
- Prefix/key tables, status line, layout algorithms, copy mode, choose tree.

6. Phase 5: Compatibility hardening
- Track coverage for all 90 commands.
- Add golden tests against tmux behavior for command outputs and edge cases.

## Practical rules for parity

- Keep wire protocol internal; optimize for behavior compatibility, not implementation similarity.
- Port behavior in vertical slices (command + state + tests), not subsystem-by-subsystem.
- Validate each command against tmux using fixture-driven tests.
