# tmux Command Coverage Tracker

Generated from: `tmux/cmd.c`

Status legend:
- ✅ REAL — changes core state / runtime in meaningful way
- 🟡 PARTIAL — tracks state in compat BTreeMap, no runtime effect
- ❌ STUB — appends to prompt_history, returns CommandAccepted, no behavior

## REAL (32)

- ✅ new_session — `dmux-core/src/lib.rs:102`
- ✅ kill_session — `dmux-core/src/lib.rs:277`
- ✅ attach_session — `dmux-core/src/lib.rs:141`
- ✅ switch_client — `dmux-core/src/lib.rs:149`
- ✅ detach_client — `dmux-core/src/lib.rs:158`
- ✅ new_window — `dmux-core/src/lib.rs:311`
- ✅ kill_window — `dmux-core/src/lib.rs:379`
- ✅ select_window — `dmux-core/src/lib.rs:403`
- ✅ next_window — `dmux-core/src/lib.rs:417`
- ✅ previous_window — `dmux-core/src/lib.rs:433`
- ✅ last_window — `dmux-core/src/lib.rs:453`
- ✅ rename_session — `dmux-core/src/lib.rs:345`
- ✅ rename_window — `dmux-core/src/lib.rs:365`
- ✅ split_window — `dmux-core/src/lib.rs:469`
- ✅ kill_pane — `dmux-core/src/lib.rs:499`
- ✅ select_pane — `dmux-core/src/lib.rs:525`
- ✅ last_pane — `dmux-core/src/lib.rs:545`
- ✅ send_keys — `dmux-core/src/lib.rs:569`
- ✅ kill_server — `dmux-core/src/lib.rs:292`
- ✅ start_server — `dmux-core/src/lib.rs:307`
- ✅ lock_client — `dmux-core/src/lib.rs:192`
- ✅ lock_server — `dmux-core/src/lib.rs:216`
- ✅ lock_session — `dmux-core/src/lib.rs:221`
- ✅ suspend_client — `dmux-core/src/lib.rs:204`
- ✅ list_sessions — `dmux-server/src/lib.rs:757`
- ✅ list_windows — `dmux-core/src/lib.rs:617`
- ✅ list_panes — `dmux-core/src/lib.rs:639`
- ✅ list_clients — `dmux-core/src/lib.rs:181`
- ✅ has_session — `dmux-core/src/lib.rs:341`
- ✅ display_message — `dmux-server/src/lib.rs:412`
- ✅ resize_pane — `dmux-core/src/lib.rs:resize_pane`; PTY TIOCSWINSZ via `PtyRuntime::resize_pty`
- ✅ capture_pane — server reads real PTY output via `output_snapshot` + `set_pane_output`

## PARTIAL (22)

State stored, no runtime behavior. Listing/reading works; effects do not.

- 🟡 bind_key / unbind_key / list_keys — `dmux-server/src/lib.rs:946`
- 🟡 set_buffer / load_buffer — `dmux-server/src/lib.rs:976`
- 🟡 delete_buffer — `dmux-server/src/lib.rs:987`
- 🟡 list_buffers — `dmux-server/src/lib.rs:999`
- 🟡 show_buffer / paste_buffer / save_buffer — `dmux-server/src/lib.rs:1006`
- 🟡 set_hook / show_hooks — `dmux-server/src/lib.rs:1019`
- 🟡 set_window_option / show_window_options — `dmux-server/src/lib.rs:1037`
- 🟡 show_messages — `dmux-server/src/lib.rs:418`
- 🟡 set_option / show_options — `dmux-server/src/lib.rs:422`
- 🟡 set_environment / show_environment — `dmux-server/src/lib.rs:437`
- 🟡 show_prompt_history / clear_prompt_history — `dmux-server/src/lib.rs:1064`
- 🟡 wait_for — `dmux-server/src/lib.rs:1127`

## STUB (38)

Appends to compat.prompt_history at `dmux-server/src/lib.rs:1071`, returns CommandAccepted with no effect:

- ❌ display_menu, display_panes, display_popup
- ❌ clock_mode, copy_mode, customize_mode
- ❌ confirm_before, command_prompt
- ❌ refresh_client, select_layout, send_prefix
- ❌ server_access, source_file
- ❌ swap_pane, swap_window, break_pane
- ❌ choose_buffer, choose_client, choose_tree
- ❌ if_shell, join_pane
- ❌ link_window, unlink_window, move_pane, move_window
- ❌ next_layout, previous_layout
- ❌ pipe_pane
- ❌ resize_window
- ❌ respawn_pane, respawn_window
- ❌ rotate_window, run_shell
- ❌ clear_history — `dmux-server/src/lib.rs:1111`
- ❌ find_window — read-only query (`dmux-server/src/lib.rs:1114`)
- ❌ list_commands — hardcoded list (`dmux-server/src/lib.rs:799`)

## Summary

Total 90 commands.
- 32 real (36%)
- 22 partial / compat-tracking (24%)
- 36 stub (40%)

## Priority gaps (high value, currently STUB)

1. **resize_pane / resize_window** — required for usable multi-pane layout once visible.
2. **swap_pane / rotate_window** — pane reorganization.
3. **break_pane / join_pane** — move panes across windows.
4. **capture_pane** — proper PTY output capture (currently reads `last_input`).
5. **clear_history** — pane scrollback management.
6. **resize via SIGWINCH + TIOCSWINSZ** — server still hardcodes 40×120.
7. **copy_mode** server-side (UI has its own copy mode in `dmux/src/main.rs`; the tmux-command is a stub).
