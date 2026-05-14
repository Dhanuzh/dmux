# dmux-rs

Rust rewrite of tmux. Terminal multiplexer with sessions, windows, panes, layouts, copy mode, floating panes, mouse, session resurrect.

Crate: [`dmux-rs`](https://crates.io/crates/dmux-rs). Binary: `dmux`.

## Install

```bash
cargo install dmux-rs
```

Binary lands as `dmux`. Start the server, open the UI:

```bash
dmux start-server &
dmux ui --session work
```

## Keys (tmux compatible)

Prefix: `Ctrl-b`

| Key   | Action |
|-------|--------|
| `c`   | new window |
| `"`   | horizontal split |
| `%`   | vertical split |
| `n` / `p` | next / prev window |
| `0`-`9` | select window |
| `o`   | next pane |
| `;`   | last pane |
| `x`   | kill pane |
| arrow | resize pane |
| `[`   | copy mode |
| `]`   | paste yanked |
| `f`   | toggle floating |
| `P`   | run-shell popup |
| `?`   | keys help popup |
| `:`   | command prompt |
| `d`   | detach |
| `q`   | quit UI |

Mouse: scroll wheel scrolls scrollback. Left-click selects pane.

## Config

`~/.dmux.conf`. Lines like `set <key> <value>`:

```
set status-left "[#S]"
set status-right "#T #H"
set status-bg blue
set status-fg white
```

Placeholders: `#S` session, `#W` window, `#P` pane, `#H` hostname, `#T` HH:MM, `#(cmd)` shell.

## Commands

90 tmux commands. Most real, some compat-tracked.

```bash
dmux start-server                # daemon
dmux ui --session work           # attach UI
dmux new-session work
dmux new-window --session work
dmux split-window --session work
dmux send-keys --session work --window 0 --pane 1 'echo hi' Enter
dmux list-sessions
dmux list-windows --session work
dmux list-panes --session work
dmux resize-pane --session work --pane 1 -U 5
dmux capture-pane --session work --pane 1 --lines 50
dmux kill-session work
```

Raw tmux commands work via `send-cmd`:

```bash
dmux send-cmd "select-layout tiled"
dmux send-cmd "swap-pane -s 1 -t 2"
dmux send-cmd "resize-window -t work:0 -x 200 -y 60"
dmux send-cmd "source-file ~/.dmux.conf"
dmux send-cmd "pipe-pane -o 'cat >> /tmp/log'"
dmux send-cmd "run-shell 'date'"
```

## Session resurrect

State persists to `$XDG_STATE_HOME/dmux/state.json` (or `~/.local/state/dmux/state.json`). On `start-server`, prior sessions and PTYs respawn.

## Workspace

- `dmux-rs`: CLI + UI.
- `dmux-rs-server`: PTY supervisor + dispatch.
- `dmux-rs-client`: UDS request helper.
- `dmux-rs-core`: state model.
- `dmux-rs-proto`: wire types.

## Status

| Category | Real | Compat-tracked | Stub |
|----------|------|----------------|------|
| Session  | 11   | 0              | 0    |
| Window   | 9    | 2              | 3    |
| Pane     | 9    | 0              | 2    |
| Layout   | 3    | 0              | 0    |
| Buffer   | 0    | 6              | 0    |
| Hook/option | 0  | 6              | 0    |
| Other    | 6    | 4              | 14   |

See `docs/tmux-command-coverage.md` for the full table.

## License

MIT.
