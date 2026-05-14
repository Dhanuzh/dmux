mod dmux_proto;
mod dmux_core;
mod dmux_client;
mod dmux_server;

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use crossterm::style::{Color, ResetColor, SetBackgroundColor, SetForegroundColor};
use crossterm::terminal::{self};
use crossterm::{cursor, execute};
use crate::dmux_client::send_request;
use crate::dmux_proto::{PaneInfo, Request, Response, SessionInfo, WindowInfo};

#[derive(Debug, Parser)]
#[command(name = "dmux")]
#[command(about = "A Rust rewrite scaffold for tmux", long_about = None)]
struct Cli {
    #[arg(long, global = true, default_value = "/tmp/dmux.sock")]
    socket: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    StartServer,
    Ui {
        #[arg(long)]
        session: Option<String>,
    },
    Ping,
    #[command(name = "new")]
    New {
        #[arg(short = 's', long = "session")]
        session: String,
    },
    HasSession {
        name: String,
    },
    AttachSession {
        #[arg(long)]
        session: Option<String>,
    },
    ListClients,
    LockClient,
    LockServer,
    LockSession {
        #[arg(long)]
        session: Option<String>,
    },
    SuspendClient,
    DisplayMessage {
        message: String,
    },
    ShowMessages,
    SetOption {
        name: String,
        value: String,
    },
    ShowOptions {
        #[arg(long)]
        name: Option<String>,
    },
    SetEnvironment {
        name: String,
        value: String,
    },
    ShowEnvironment {
        #[arg(long)]
        name: Option<String>,
    },
    SwitchClient {
        #[arg(long)]
        session: Option<String>,
    },
    DetachClient {
        #[arg(long)]
        session: Option<String>,
    },
    NewSession {
        name: String,
    },
    KillSession {
        name: String,
    },
    RenameSession {
        name: String,
        new_name: String,
    },
    NewWindow {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        name: Option<String>,
    },
    RenameWindow {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        new_name: String,
    },
    KillWindow {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
    },
    NextWindow {
        #[arg(long)]
        session: Option<String>,
    },
    PreviousWindow {
        #[arg(long)]
        session: Option<String>,
    },
    LastWindow {
        #[arg(long)]
        session: Option<String>,
    },
    SelectWindow {
        window: String,
        #[arg(long)]
        session: Option<String>,
    },
    SplitWindow {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        title: Option<String>,
        #[arg(short = 'h', long, conflicts_with = "vertical")]
        horizontal: bool,
        #[arg(short = 'v', long, conflicts_with = "horizontal")]
        vertical: bool,
    },
    KillPane {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        pane: Option<String>,
    },
    SelectPane {
        pane: String,
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
    },
    LastPane {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
    },
    ResizePane {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(short = 'U', long, conflicts_with_all = ["down", "left", "right"])]
        up: bool,
        #[arg(short = 'D', long, conflicts_with_all = ["up", "left", "right"])]
        down: bool,
        #[arg(short = 'L', long, conflicts_with_all = ["up", "down", "right"])]
        left: bool,
        #[arg(short = 'R', long, conflicts_with_all = ["up", "down", "left"])]
        right: bool,
        #[arg(default_value = "1")]
        amount: u16,
    },
    CapturePane {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        lines: Option<usize>,
    },
    SendKeys {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
        #[arg(long)]
        pane: Option<String>,
        #[arg(long)]
        literal: bool,
        #[arg(required = true)]
        keys: Vec<String>,
    },
    ListSessions,
    ListCommands,
    ListWindows {
        #[arg(long)]
        session: Option<String>,
    },
    ListPanes {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        window: Option<String>,
    },
    SendCmd {
        command: String,
    },
    KillServer,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::StartServer => {
            println!("starting dmux server on {}", cli.socket.display());
            dmux_server::run(&cli.socket)?;
        }
        Command::Ui { session } => {
            run_ui(&cli.socket, session)?;
        }
        Command::Ping => {
            let response = send_request(&cli.socket, Request::Ping)?;
            print_response(response)?;
        }
        Command::New { session } => {
            let create = send_request(
                &cli.socket,
                Request::CreateSession {
                    name: session.clone(),
                },
            )?;
            match &create {
                Response::SessionCreated { .. } => {
                    print_response(create)?;
                }
                Response::Error { message } if message.contains("already exists") => {
                    println!("session already exists, attaching");
                }
                _ => print_response(create)?,
            }

            let attach = send_request(
                &cli.socket,
                Request::AttachSession {
                    session: Some(session.clone()),
                },
            )?;
            print_response(attach)?;
            run_ui(&cli.socket, Some(session))?;
        }
        Command::HasSession { name } => {
            let response = send_request(&cli.socket, Request::HasSession { name })?;
            print_response(response)?;
        }
        Command::AttachSession { session } => {
            let response = send_request(&cli.socket, Request::AttachSession { session })?;
            let session_name = response_session_name(&response);
            print_response(response)?;
            run_ui(&cli.socket, session_name)?;
        }
        Command::ListClients => {
            let response = send_request(&cli.socket, Request::ListClients)?;
            print_response(response)?;
        }
        Command::LockClient => {
            let response = send_request(&cli.socket, Request::LockClient)?;
            print_response(response)?;
        }
        Command::LockServer => {
            let response = send_request(&cli.socket, Request::LockServer)?;
            print_response(response)?;
        }
        Command::LockSession { session } => {
            let response = send_request(&cli.socket, Request::LockSession { session })?;
            print_response(response)?;
        }
        Command::SuspendClient => {
            let response = send_request(&cli.socket, Request::SuspendClient)?;
            print_response(response)?;
        }
        Command::DisplayMessage { message } => {
            let response = send_request(&cli.socket, Request::DisplayMessage { message })?;
            print_response(response)?;
        }
        Command::ShowMessages => {
            let response = send_request(&cli.socket, Request::ShowMessages)?;
            print_response(response)?;
        }
        Command::SetOption { name, value } => {
            let response = send_request(&cli.socket, Request::SetOption { name, value })?;
            print_response(response)?;
        }
        Command::ShowOptions { name } => {
            let response = send_request(&cli.socket, Request::ShowOptions { name })?;
            print_response(response)?;
        }
        Command::SetEnvironment { name, value } => {
            let response = send_request(&cli.socket, Request::SetEnvironment { name, value })?;
            print_response(response)?;
        }
        Command::ShowEnvironment { name } => {
            let response = send_request(&cli.socket, Request::ShowEnvironment { name })?;
            print_response(response)?;
        }
        Command::SwitchClient { session } => {
            let response = send_request(&cli.socket, Request::SwitchClient { session })?;
            print_response(response)?;
        }
        Command::DetachClient { session } => {
            let response = send_request(&cli.socket, Request::DetachClient { session })?;
            print_response(response)?;
        }
        Command::NewSession { name } => {
            let response = send_request(&cli.socket, Request::CreateSession { name })?;
            print_response(response)?;
        }
        Command::KillSession { name } => {
            let response = send_request(&cli.socket, Request::KillSession { name })?;
            print_response(response)?;
        }
        Command::RenameSession { name, new_name } => {
            let response = send_request(&cli.socket, Request::RenameSession { name, new_name })?;
            print_response(response)?;
        }
        Command::NewWindow { session, name } => {
            let response = send_request(&cli.socket, Request::CreateWindow { session, name })?;
            print_response(response)?;
        }
        Command::RenameWindow {
            session,
            window,
            new_name,
        } => {
            let response = send_request(
                &cli.socket,
                Request::RenameWindow {
                    session,
                    window,
                    new_name,
                },
            )?;
            print_response(response)?;
        }
        Command::KillWindow { session, window } => {
            let response = send_request(&cli.socket, Request::KillWindow { session, window })?;
            print_response(response)?;
        }
        Command::NextWindow { session } => {
            let response = send_request(&cli.socket, Request::NextWindow { session })?;
            print_response(response)?;
        }
        Command::PreviousWindow { session } => {
            let response = send_request(&cli.socket, Request::PreviousWindow { session })?;
            print_response(response)?;
        }
        Command::LastWindow { session } => {
            let response = send_request(&cli.socket, Request::LastWindow { session })?;
            print_response(response)?;
        }
        Command::SelectWindow { window, session } => {
            let response = send_request(&cli.socket, Request::SelectWindow { session, window })?;
            print_response(response)?;
        }
        Command::SplitWindow {
            session,
            window,
            title,
            horizontal,
            vertical,
        } => {
            let orientation = if vertical {
                dmux_proto::SplitOrientation::Vertical
            } else {
                let _ = horizontal;
                dmux_proto::SplitOrientation::Horizontal
            };
            let response = send_request(
                &cli.socket,
                Request::SplitWindow {
                    session,
                    window,
                    title,
                    orientation,
                },
            )?;
            print_response(response)?;
        }
        Command::KillPane {
            session,
            window,
            pane,
        } => {
            let response = send_request(
                &cli.socket,
                Request::KillPane {
                    session,
                    window,
                    pane,
                },
            )?;
            print_response(response)?;
        }
        Command::SelectPane {
            pane,
            session,
            window,
        } => {
            let response = send_request(
                &cli.socket,
                Request::SelectPane {
                    session,
                    window,
                    pane,
                },
            )?;
            print_response(response)?;
        }
        Command::LastPane { session, window } => {
            let response = send_request(&cli.socket, Request::LastPane { session, window })?;
            print_response(response)?;
        }
        Command::ResizePane {
            session,
            window,
            pane,
            up,
            down,
            left,
            right,
            amount,
        } => {
            let direction = if up {
                dmux_proto::ResizeDir::Up
            } else if left {
                dmux_proto::ResizeDir::Left
            } else if right {
                dmux_proto::ResizeDir::Right
            } else {
                dmux_proto::ResizeDir::Down
            };
            let _ = down;
            let response = send_request(
                &cli.socket,
                Request::ResizePane {
                    session,
                    window,
                    pane,
                    direction,
                    amount,
                },
            )?;
            print_response(response)?;
        }
        Command::CapturePane {
            session,
            window,
            pane,
            lines,
        } => {
            let response = send_request(
                &cli.socket,
                Request::CapturePane {
                    session,
                    window,
                    pane,
                    lines,
                },
            )?;
            print_response(response)?;
        }
        Command::SendKeys {
            session,
            window,
            pane,
            keys,
            literal,
        } => {
            let response = send_request(
                &cli.socket,
                Request::SendKeys {
                    session,
                    window,
                    pane,
                    keys,
                    literal,
                },
            )?;
            print_response(response)?;
        }
        Command::ListSessions => {
            let response = send_request(&cli.socket, Request::ListSessions)?;
            print_response(response)?;
        }
        Command::ListCommands => {
            let response = send_request(&cli.socket, Request::ListCommands)?;
            print_response(response)?;
        }
        Command::ListWindows { session } => {
            let response = send_request(&cli.socket, Request::ListWindows { session })?;
            print_response(response)?;
        }
        Command::ListPanes { session, window } => {
            let response = send_request(&cli.socket, Request::ListPanes { session, window })?;
            print_response(response)?;
        }
        Command::SendCmd { command } => {
            let response = send_request(&cli.socket, Request::ExecuteRaw { command })?;
            print_response(response)?;
        }
        Command::KillServer => {
            let response = send_request(&cli.socket, Request::KillServer)?;
            print_response(response)?;
        }
    }

    Ok(())
}

fn response_session_name(response: &Response) -> Option<String> {
    match response {
        Response::SessionAttached { name } => Some(name.clone()),
        _ => None,
    }
}

#[derive(Debug, Default)]
struct UiSnapshot {
    attached_session: Option<String>,
    sessions: Vec<SessionInfo>,
    windows: Vec<WindowInfo>,
    panes: Vec<PaneInfo>,
}

#[derive(Debug, Clone)]
struct UiRenderFrame {
    content_lines: Vec<String>,
}

impl Default for UiRenderFrame {
    fn default() -> Self {
        Self {
            content_lines: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct UiConfig {
    status_left: Option<String>,
    status_right: Option<String>,
    status_bg: Option<String>,
    status_fg: Option<String>,
    prefix_key: Option<String>,
}

fn load_ui_config() -> UiConfig {
    let mut cfg = UiConfig::default();
    let Some(home) = std::env::var_os("HOME") else { return cfg };
    let home = std::path::PathBuf::from(home);
    let candidates = [
        home.join(".dmux.conf"),
        home.join(".config/dmux/dmux.conf"),
        home.join(".tmux.conf"),
        home.join(".config/tmux/tmux.conf"),
    ];
    let mut contents = String::new();
    for path in candidates {
        if let Ok(c) = std::fs::read_to_string(&path) {
            contents.push_str(&c);
            contents.push('\n');
        }
    }
    if contents.is_empty() { return cfg; }
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') { continue; }
        let rest = trimmed.strip_prefix("set ").or_else(|| trimmed.strip_prefix("set-option ")).unwrap_or(trimmed);
        let (key, value) = match rest.split_once(' ') {
            Some(kv) => kv,
            None => continue,
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "status-left" => cfg.status_left = Some(value),
            "status-right" => cfg.status_right = Some(value),
            "status-bg" => cfg.status_bg = Some(value),
            "status-fg" => cfg.status_fg = Some(value),
            "prefix" => cfg.prefix_key = Some(value),
            _ => {}
        }
    }
    cfg
}

fn substitute_status(template: &str, session: &str, window: &str, pane: &str) -> String {
    let mut out = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '#' { out.push(c); continue; }
        let Some(next) = chars.next() else { out.push('#'); break; };
        match next {
            'S' => out.push_str(session),
            'W' => out.push_str(window),
            'P' => out.push_str(pane),
            'H' => out.push_str(&std::env::var("HOSTNAME").unwrap_or_else(|_| {
                std::process::Command::new("hostname").output().ok()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default()
            })),
            'T' => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                let secs_of_day = now % 86400;
                let hh = (secs_of_day / 3600) as u8;
                let mm = ((secs_of_day % 3600) / 60) as u8;
                out.push_str(&format!("{hh:02}:{mm:02}"));
            }
            '(' => {
                let mut cmd = String::new();
                while let Some(&p) = chars.peek() {
                    chars.next();
                    if p == ')' { break; }
                    cmd.push(p);
                }
                if !cmd.is_empty() {
                    if let Ok(o) = std::process::Command::new("sh").arg("-c").arg(&cmd).output() {
                        out.push_str(String::from_utf8_lossy(&o.stdout).trim_end_matches('\n'));
                    }
                }
            }
            other => { out.push('#'); out.push(other); }
        }
    }
    out
}

fn parse_prefix_key(spec: &str) -> Option<char> {
    let s = spec.trim().to_ascii_lowercase();
    let s = s.strip_prefix("c-").or_else(|| s.strip_prefix("ctrl-")).unwrap_or(&s);
    s.chars().next()
}

fn fetch_server_prefix(socket: &std::path::Path) -> Option<String> {
    let resp = send_request(socket, Request::ShowOptions { name: Some("prefix".to_string()) }).ok()?;
    match resp {
        Response::Options(opts) => opts.into_iter().find(|nv| nv.name == "prefix").map(|nv| nv.value),
        _ => None,
    }
}

fn parse_color_name(s: &str) -> Option<Color> {
    Some(match s.to_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::DarkRed,
        "green" => Color::DarkGreen,
        "yellow" => Color::DarkYellow,
        "blue" => Color::DarkBlue,
        "magenta" => Color::DarkMagenta,
        "cyan" => Color::DarkCyan,
        "white" => Color::White,
        "grey" | "gray" => Color::DarkGrey,
        "brightred" => Color::Red,
        "brightgreen" => Color::Green,
        "brightyellow" => Color::Yellow,
        "brightblue" => Color::Blue,
        "brightmagenta" => Color::Magenta,
        "brightcyan" => Color::Cyan,
        _ => return None,
    })
}

#[derive(Debug, Clone)]
struct PopupState {
    title: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct CopyModeState {
    cursor_row: usize,
    cursor_col: usize,
    anchor: Option<(usize, usize)>,
}

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(
            std::io::stdout(),
            terminal::EnterAlternateScreen,
            EnableMouseCapture,
            cursor::Hide
        )?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            DisableMouseCapture,
            terminal::LeaveAlternateScreen,
            cursor::Show
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn run_ui(socket: &std::path::Path, session: Option<String>) -> Result<()> {
    if session.is_some() {
        let response = send_request(
            socket,
            Request::AttachSession {
                session: session.clone(),
            },
        )?;
        if let Response::Error { message } = response {
            bail!(message);
        }
    }

    let _guard = TerminalGuard::enter()?;
    let ui_config = load_ui_config();
    let server_prefix = fetch_server_prefix(socket);
    let prefix_char = ui_config.prefix_key.as_deref()
        .or(server_prefix.as_deref())
        .and_then(parse_prefix_key)
        .unwrap_or('b');
    let mut prefix = false;
    let mut show_meta = false;
    let mut command_mode: Option<String> = None;
    let mut notice: Option<String> = None;
    let mut scroll_offset = 0usize;
    let mut scroll_mode = false;
    let mut copy_mode: Option<CopyModeState> = None;
    let mut yank_buffer = String::new();
    let mut frame = UiRenderFrame::default();
    let mut last_signature: Option<u64> = None;
    let mut last_term_size: Option<(u16, u16)> = None;
    let mut popup: Option<PopupState> = None;
    let mut popup_input: Option<String> = None;

    loop {
        let snapshot = fetch_snapshot(socket)?;
        let (term_cols, term_rows) = terminal::size().unwrap_or((120, 40));
        if last_term_size != Some((term_cols, term_rows)) {
            if let Err(error) = sync_active_window_size(socket, &snapshot, term_cols, term_rows) {
                notice = Some(format!("resize sync error: {error}"));
            }
            last_term_size = Some((term_cols, term_rows));
        }
        let signature = render_signature(
            &snapshot,
            prefix,
            show_meta,
            command_mode.as_deref(),
            notice.as_deref(),
            scroll_offset,
            scroll_mode,
            copy_mode.as_ref(),
            popup.as_ref(),
            popup_input.as_deref(),
        );
        if last_signature != Some(signature) {
            frame = render_ui(
                &snapshot,
                prefix,
                show_meta,
                command_mode.as_deref(),
                notice.as_deref(),
                scroll_offset,
                scroll_mode,
                copy_mode.as_ref(),
                popup.as_ref(),
                popup_input.as_deref(),
                &ui_config,
            )?;
            last_signature = Some(signature);
        }

        if event::poll(Duration::from_millis(200))? {
            match event::read()? {
                Event::Mouse(me) => {
                    handle_mouse(socket, &snapshot, me, &mut scroll_offset, &mut scroll_mode)?;
                    continue;
                }
                Event::Resize(cols, rows) => {
                    if let Err(error) = sync_active_window_size(socket, &snapshot, cols, rows) {
                        notice = Some(format!("resize sync error: {error}"));
                    }
                    last_term_size = Some((cols, rows));
                    continue;
                }
                Event::Key(key) => {
                    if popup.is_some() {
                        if matches!(key.code, KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q')) {
                            popup = None;
                        }
                        continue;
                    }
                    if let Some(buf) = popup_input.as_mut() {
                        match key.code {
                            KeyCode::Esc => {
                                popup_input = None;
                                notice = Some("popup cancelled".to_string());
                            }
                            KeyCode::Enter => {
                                let cmd = buf.trim().to_string();
                                popup_input = None;
                                if !cmd.is_empty() {
                                    let response = send_request(
                                        socket,
                                        Request::ExecuteRaw { command: format!("run-shell {cmd}") },
                                    )?;
                                    match response {
                                        Response::Commands(lines) => {
                                            popup = Some(PopupState { title: cmd, lines });
                                        }
                                        Response::Error { message } => {
                                            notice = Some(format!("run-shell error: {message}"));
                                        }
                                        _ => notice = Some("run-shell: no output".to_string()),
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                let _ = buf.pop();
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                buf.push(c);
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(buf) = command_mode.as_mut() {
                        match key.code {
                            KeyCode::Esc => {
                                command_mode = None;
                                notice = Some("command cancelled".to_string());
                            }
                            KeyCode::Enter => {
                                let command = buf.trim().to_string();
                                command_mode = None;
                                if !command.is_empty() {
                                    let response =
                                        send_request(socket, Request::ExecuteRaw { command })?;
                                    match response {
                                        Response::Error { message } => {
                                            notice = Some(format!("error: {message}"))
                                        }
                                        Response::CommandAccepted { command } => {
                                            notice = Some(format!("ok: {command}"))
                                        }
                                        _ => notice = Some("command executed".to_string()),
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                let _ = buf.pop();
                            }
                            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                                buf.push(c);
                            }
                            _ => {}
                        }
                        continue;
                    }

                    if let Some(copy) = copy_mode.as_mut() {
                        match key.code {
                            KeyCode::Esc | KeyCode::Char('q') => {
                                copy_mode = None;
                                scroll_mode = false;
                                scroll_offset = 0;
                                notice = Some("copy mode off".to_string());
                            }
                            KeyCode::Up => {
                                if copy.cursor_row > 0 {
                                    copy.cursor_row -= 1;
                                } else {
                                    scroll_mode = true;
                                    scroll_offset = scroll_offset.saturating_add(1);
                                }
                            }
                            KeyCode::Down => {
                                if copy.cursor_row + 1 < frame.content_lines.len() {
                                    copy.cursor_row += 1;
                                } else if scroll_offset > 0 {
                                    scroll_offset = scroll_offset.saturating_sub(1);
                                    if scroll_offset == 0 {
                                        scroll_mode = false;
                                    }
                                }
                            }
                            KeyCode::Left => {
                                copy.cursor_col = copy.cursor_col.saturating_sub(1);
                            }
                            KeyCode::Right => {
                                copy.cursor_col = copy.cursor_col.saturating_add(1);
                            }
                            KeyCode::Home => copy.cursor_col = 0,
                            KeyCode::End => {
                                let max_col = frame
                                    .content_lines
                                    .get(copy.cursor_row)
                                    .map(|l| l.chars().count())
                                    .unwrap_or(0);
                                copy.cursor_col = max_col.saturating_sub(1);
                            }
                            KeyCode::PageUp => {
                                scroll_mode = true;
                                scroll_offset = scroll_offset.saturating_add(10);
                            }
                            KeyCode::PageDown => {
                                scroll_offset = scroll_offset.saturating_sub(10);
                                if scroll_offset == 0 {
                                    scroll_mode = false;
                                }
                            }
                            KeyCode::Char('v') => {
                                if copy.anchor.is_some() {
                                    copy.anchor = None;
                                    notice = Some("selection cleared".to_string());
                                } else {
                                    copy.anchor = Some((copy.cursor_row, copy.cursor_col));
                                    notice = Some("selection started".to_string());
                                }
                            }
                            KeyCode::Enter => {
                                if let Some(yanked) = yank_selection(&frame.content_lines, copy) {
                                    yank_buffer = yanked.clone();
                                    notice = Some(format!(
                                        "yanked {} chars",
                                        yank_buffer.chars().count()
                                    ));
                                } else if let Some(line) = frame.content_lines.get(copy.cursor_row)
                                {
                                    yank_buffer = line.clone();
                                    notice = Some(format!(
                                        "yanked line ({} chars)",
                                        yank_buffer.chars().count()
                                    ));
                                }
                                copy_mode = None;
                                scroll_mode = false;
                                scroll_offset = 0;
                            }
                            _ => {}
                        }

                        if let Some(copy) = copy_mode.as_mut() {
                            if frame.content_lines.is_empty() {
                                copy.cursor_row = 0;
                                copy.cursor_col = 0;
                            } else {
                                let max_row = frame.content_lines.len().saturating_sub(1);
                                copy.cursor_row = copy.cursor_row.min(max_row);
                                let max_col = frame
                                    .content_lines
                                    .get(copy.cursor_row)
                                    .map(|l| l.chars().count())
                                    .unwrap_or(0);
                                copy.cursor_col = copy.cursor_col.min(max_col.saturating_sub(1));
                            }
                        }
                        continue;
                    }

                    if prefix {
                        if key.code == KeyCode::Char('q') {
                            break;
                        }
                        if key.code == KeyCode::Char('m') {
                            show_meta = !show_meta;
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char(':') {
                            command_mode = Some(String::new());
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char('d') {
                            break;
                        }
                        if key.code == KeyCode::Char('[') {
                            copy_mode = Some(CopyModeState {
                                cursor_row: frame.content_lines.len().saturating_sub(1),
                                cursor_col: 0,
                                anchor: None,
                            });
                            scroll_mode = true;
                            notice = Some(
                                "copy mode on (arrows move, v select, Enter yank, Esc exit)"
                                    .to_string(),
                            );
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char(']') {
                            if !yank_buffer.is_empty() {
                                let Some((session_name, window_name, pane_id)) =
                                    active_pane_target(&snapshot)
                                else {
                                    prefix = false;
                                    continue;
                                };
                                let response = send_request(
                                    socket,
                                    Request::SendKeys {
                                        session: Some(session_name),
                                        window: Some(window_name),
                                        pane: Some(pane_id),
                                        keys: vec![yank_buffer.clone()],
                                        literal: true,
                                    },
                                )?;
                                if let Response::Error { message } = response {
                                    notice = Some(format!("paste error: {message}"));
                                } else {
                                    notice = Some("pasted yanked text".to_string());
                                }
                            } else {
                                notice = Some("yank buffer empty".to_string());
                            }
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char('o') {
                            select_next_pane(socket, &snapshot)?;
                            scroll_offset = 0;
                            scroll_mode = false;
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char('P') {
                            popup_input = Some(String::new());
                            notice = Some("popup: enter shell command, Enter to run, Esc to cancel".to_string());
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char('f') {
                            let Some((session_name, window_name, pane_id)) = active_pane_target(&snapshot) else {
                                prefix = false;
                                continue;
                            };
                            let resp = send_request(socket, Request::ToggleFloating {
                                session: Some(session_name),
                                window: Some(window_name),
                                pane: Some(pane_id),
                            })?;
                            notice = response_notice(&resp, "toggled floating");
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char('?') {
                            popup = Some(PopupState {
                                title: "dmux keys".to_string(),
                                lines: vec![
                                    "C-b c    new window".to_string(),
                                    "C-b \"    horizontal split".to_string(),
                                    "C-b %    vertical split".to_string(),
                                    "C-b n/p  next/prev window".to_string(),
                                    "C-b 0-9  select window".to_string(),
                                    "C-b o    next pane".to_string(),
                                    "C-b ;    last pane".to_string(),
                                    "C-b x    kill pane".to_string(),
                                    "C-b arr  resize pane".to_string(),
                                    "C-b [    copy mode".to_string(),
                                    "C-b ]    paste".to_string(),
                                    "C-b P    run-shell popup".to_string(),
                                    "C-b ?    this help".to_string(),
                                    "C-b :    command prompt".to_string(),
                                    "C-b d    detach".to_string(),
                                    "C-b q    quit UI".to_string(),
                                ],
                            });
                            prefix = false;
                            continue;
                        }
                        if key.code == KeyCode::Char(';') {
                            let Some((session_name, window_name, _)) =
                                active_pane_target(&snapshot)
                            else {
                                prefix = false;
                                continue;
                            };
                            let _ = send_request(
                                socket,
                                Request::LastPane {
                                    session: Some(session_name),
                                    window: Some(window_name),
                                },
                            )?;
                            scroll_offset = 0;
                            scroll_mode = false;
                            prefix = false;
                            continue;
                        }
                        if let KeyCode::Char(d) = key.code {
                            if d.is_ascii_digit() {
                                let Some((session_name, _, _)) = active_pane_target(&snapshot)
                                else {
                                    prefix = false;
                                    continue;
                                };
                                let _ = send_request(
                                    socket,
                                    Request::SelectWindow {
                                        session: Some(session_name),
                                        window: d.to_string(),
                                    },
                                )?;
                                scroll_offset = 0;
                                scroll_mode = false;
                                prefix = false;
                                continue;
                            }
                        }
                        let n = handle_prefix_key(socket, &snapshot, key.code)?;
                        if n.is_some() {
                            notice = n;
                        }
                        scroll_offset = 0;
                        scroll_mode = false;
                        prefix = false;
                        continue;
                    }

                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && key.code == KeyCode::Char(prefix_char)
                    {
                        prefix = true;
                        continue;
                    }

                    match key.code {
                        KeyCode::PageUp => {
                            scroll_mode = true;
                            scroll_offset = scroll_offset.saturating_add(10);
                            notice = Some(format!("scroll mode offset={scroll_offset}"));
                            continue;
                        }
                        KeyCode::PageDown => {
                            scroll_offset = scroll_offset.saturating_sub(10);
                            if scroll_offset == 0 {
                                scroll_mode = false;
                            }
                            notice = Some(format!("scroll mode offset={scroll_offset}"));
                            continue;
                        }
                        KeyCode::Home => {
                            scroll_mode = true;
                            scroll_offset = usize::MAX / 2;
                            notice = Some("scroll mode: top".to_string());
                            continue;
                        }
                        KeyCode::End => {
                            scroll_mode = false;
                            scroll_offset = 0;
                            notice = Some("scroll mode: live".to_string());
                            continue;
                        }
                        KeyCode::Esc if scroll_mode => {
                            scroll_mode = false;
                            scroll_offset = 0;
                            notice = Some("scroll mode off".to_string());
                            continue;
                        }
                        _ => {}
                    }

                    if scroll_offset > 0 {
                        scroll_offset = 0;
                        scroll_mode = false;
                    }

                    forward_key_to_active_pane(socket, &snapshot, key)?;
                    notice = None;
                }
                _ => {}
            };
        }
    }

    Ok(())
}

fn fetch_snapshot(socket: &std::path::Path) -> Result<UiSnapshot> {
    let clients = send_request(socket, Request::ListClients)?;
    let sessions = send_request(socket, Request::ListSessions)?;
    let windows = send_request(socket, Request::ListWindows { session: None })?;
    let panes = send_request(
        socket,
        Request::ListPanes {
            session: None,
            window: None,
        },
    )?;

    let attached_session = match clients {
        Response::Clients(clients) => clients
            .into_iter()
            .find_map(|c| c.attached_session)
            .filter(|s| !s.is_empty()),
        _ => None,
    };
    let sessions = match sessions {
        Response::Sessions(v) => v,
        Response::Error { message } => bail!(message),
        _ => vec![],
    };
    let windows = match windows {
        Response::Windows(v) => v,
        Response::Error { message } => bail!(message),
        _ => vec![],
    };
    let panes = match panes {
        Response::Panes(v) => v,
        Response::Error { message } => bail!(message),
        _ => vec![],
    };

    Ok(UiSnapshot {
        attached_session,
        sessions,
        windows,
        panes,
    })
}

fn render_signature(
    snapshot: &UiSnapshot,
    prefix: bool,
    show_meta: bool,
    command_mode: Option<&str>,
    notice: Option<&str>,
    scroll_offset: usize,
    scroll_mode: bool,
    copy_mode: Option<&CopyModeState>,
    popup: Option<&PopupState>,
    popup_input: Option<&str>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    prefix.hash(&mut hasher);
    show_meta.hash(&mut hasher);
    scroll_offset.hash(&mut hasher);
    scroll_mode.hash(&mut hasher);
    command_mode.unwrap_or_default().hash(&mut hasher);
    notice.unwrap_or_default().hash(&mut hasher);
    snapshot.attached_session.hash(&mut hasher);

    for w in &snapshot.windows {
        w.session_name.hash(&mut hasher);
        w.window_name.hash(&mut hasher);
        w.active.hash(&mut hasher);
        w.pane_count.hash(&mut hasher);
        w.layout.hash(&mut hasher);
        if let Some(tree) = &w.layout_tree {
            format!("{tree:?}").hash(&mut hasher);
        }
    }
    for p in &snapshot.panes {
        p.session_name.hash(&mut hasher);
        p.window_name.hash(&mut hasher);
        p.pane_id.hash(&mut hasher);
        p.active.hash(&mut hasher);
        if let Some(out) = &p.last_output {
            out.len().hash(&mut hasher);
            let tail = &out.as_bytes()[out.len().saturating_sub(256)..];
            tail.hash(&mut hasher);
        }
    }
    if let Some(copy) = copy_mode {
        copy.cursor_row.hash(&mut hasher);
        copy.cursor_col.hash(&mut hasher);
        copy.anchor.hash(&mut hasher);
    }
    if let Some(p) = popup {
        p.title.hash(&mut hasher);
        p.lines.len().hash(&mut hasher);
        for l in &p.lines { l.hash(&mut hasher); }
    }
    popup_input.unwrap_or("").hash(&mut hasher);
    hasher.finish()
}

fn render_ui(
    snapshot: &UiSnapshot,
    prefix: bool,
    show_meta: bool,
    command_mode: Option<&str>,
    notice: Option<&str>,
    scroll_offset: usize,
    scroll_mode: bool,
    copy_mode: Option<&CopyModeState>,
    popup: Option<&PopupState>,
    popup_input: Option<&str>,
    ui_config: &UiConfig,
) -> Result<UiRenderFrame> {
    let mut out = std::io::stdout();
    // DEC mode 2026: synchronized output — terminal buffers until end. Eliminates tearing on supporting emulators; no-op elsewhere.
    out.write_all(b"\x1b[?2026h")?;
    execute!(out, cursor::Hide, cursor::MoveTo(0, 0))?;

    let (cols, rows) = terminal::size().unwrap_or((120, 40));
    let cols_usize = cols as usize;
    let rows_usize = rows as usize;

    let attached_session = snapshot
        .attached_session
        .clone()
        .unwrap_or_else(|| "<none>".to_string());
    let attached_ref = snapshot.attached_session.as_deref();
    let active_window_in_attached = snapshot
        .windows
        .iter()
        .find(|w| w.active && attached_ref.map(|s| s == w.session_name).unwrap_or(true))
        .or_else(|| snapshot.windows.iter().find(|w| w.active));
    let active_pane = active_window_in_attached.and_then(|w| {
        snapshot
            .panes
            .iter()
            .find(|p| p.active && p.session_name == w.session_name && p.window_name == w.window_name)
    });
    let active_label = active_pane
        .map(|p| format!("{}:{}.{}", p.session_name, p.window_name, p.pane_id))
        .unwrap_or_else(|| "<none>".to_string());

    let top_bar = "dmux ui | Ctrl-b prefix | c new-window | \" split | % split | n/p window | o next-pane | ; last-pane | d detach | q quit | m meta | : command | PgUp/PgDn scroll";
    render_bar_line(&mut out, 0, cols_usize, top_bar, Color::Black, Color::Cyan)?;

    let mut content_start = 1usize;
    if show_meta {
        let meta_lines = build_meta_lines(snapshot);
        for line in meta_lines
            .iter()
            .take(rows_usize.saturating_sub(3))
            .enumerate()
        {
            render_fixed_line(&mut out, 1 + line.0, cols_usize, line.1)?;
        }
        content_start = (meta_lines.len() + 2).min(rows_usize.saturating_sub(2));
    }

    let window_bar = build_window_pane_bar(snapshot);
    let sess_name = snapshot.attached_session.clone().unwrap_or_else(|| "-".into());
    let active_win = snapshot.windows.iter().find(|w| w.active).map(|w| w.window_name.clone()).unwrap_or_default();
    let active_pane_id = snapshot.panes.iter().find(|p| p.active).map(|p| p.pane_id.to_string()).unwrap_or_default();
    let left = ui_config.status_left.as_deref().map(|t| substitute_status(t, &sess_name, &active_win, &active_pane_id));
    let right = ui_config.status_right.as_deref().map(|t| substitute_status(t, &sess_name, &active_win, &active_pane_id));
    let bar_text = match (left, right) {
        (Some(l), Some(r)) => {
            let pad = cols_usize.saturating_sub(l.chars().count() + r.chars().count() + window_bar.chars().count());
            format!("{l} {window_bar}{}{r}", " ".repeat(pad))
        }
        (Some(l), None) => format!("{l} {window_bar}"),
        (None, Some(r)) => format!("{window_bar} {r}"),
        (None, None) => window_bar.clone(),
    };
    let bar_bg = ui_config.status_bg.as_deref().and_then(parse_color_name).unwrap_or(Color::DarkGreen);
    let bar_fg = ui_config.status_fg.as_deref().and_then(parse_color_name).unwrap_or(Color::Black);
    render_bar_line(
        &mut out,
        rows_usize.saturating_sub(2),
        cols_usize,
        &bar_text,
        bar_fg,
        bar_bg,
    )?;

    let status_line = if let Some(copy) = copy_mode {
        format!(
            "COPY row={} col={} anchor={} scroll={} (v select, Enter yank, Esc exit)",
            copy.cursor_row,
            copy.cursor_col,
            copy.anchor
                .map(|(r, c)| format!("{r}:{c}"))
                .unwrap_or_else(|| "none".to_string()),
            scroll_offset
        )
    } else if let Some(input) = command_mode {
        format!(":{}", input)
    } else if let Some(msg) = notice {
        format!(
            "session={} pane={} prefix={} meta={} scroll={} | {}",
            attached_session,
            active_label,
            if prefix { "ON" } else { "off" },
            if show_meta { "ON" } else { "off" },
            scroll_offset,
            msg
        )
    } else if scroll_mode || scroll_offset > 0 {
        format!(
            "session={} pane={} prefix={} meta={} | SCROLL offset={} (PgUp/PgDn/Home/End/Esc)",
            attached_session,
            active_label,
            if prefix { "ON" } else { "off" },
            if show_meta { "ON" } else { "off" },
            scroll_offset
        )
    } else {
        format!(
            "session={} pane={} prefix={} meta={} (typing goes to active pane)",
            attached_session,
            active_label,
            if prefix { "ON" } else { "off" },
            if show_meta { "ON" } else { "off" }
        )
    };
    let status_bg = if copy_mode.is_some() {
        Color::DarkYellow
    } else if prefix {
        Color::DarkBlue
    } else if scroll_mode || scroll_offset > 0 {
        Color::DarkMagenta
    } else {
        Color::DarkGrey
    };
    render_bar_line(
        &mut out,
        rows_usize.saturating_sub(1),
        cols_usize,
        &status_line,
        Color::White,
        status_bg,
    )?;

    let content_height = rows_usize.saturating_sub(content_start + 2).max(1);

    // Gather panes in active window and render according to split orientation.
    let active_window_info = active_window_in_attached;
    let active_window_key = active_window_info.map(|w| (w.session_name.clone(), w.window_name.clone()));
    let active_window_layout = active_window_info
        .map(|win| win.layout)
        .unwrap_or(dmux_proto::SplitOrientation::Horizontal);
    let active_window_tree = active_window_info.and_then(|win| win.layout_tree.as_ref());
    let window_panes: Vec<&PaneInfo> = if let Some((s, w)) = active_window_key.as_ref() {
        let mut v: Vec<&PaneInfo> = snapshot
            .panes
            .iter()
            .filter(|p| &p.session_name == s && &p.window_name == w)
            .collect();
        v.sort_by_key(|p| p.pane_id);
        v
    } else {
        Vec::new()
    };

    let mut active_styled: Vec<StyledLine> = Vec::new();
    let mut active_cursor_screen: Option<(usize, usize)> = None;

    if window_panes.is_empty() {
        let placeholder = vec![styled_from_str(
            "<shell ready: start typing in this window>",
        )];
        write_pane_region_at(
            &mut out,
            content_start,
            content_height,
            0,
            cols_usize,
            &placeholder,
            copy_mode,
        )?;
        active_styled = placeholder;
    } else if let Some(tree) = active_window_tree {
        for row in content_start..(content_start + content_height) {
            render_fixed_line(&mut out, row, cols_usize, "")?;
        }
        draw_split_borders(tree, 0, content_start, cols_usize, content_height, &mut out)?;
        let mut rects = Vec::new();
        build_split_rects(
            tree,
            0,
            content_start,
            cols_usize,
            content_height,
            &mut rects,
        );
        for rect in rects {
            let Some(pane) = window_panes.iter().find(|p| p.pane_id == rect.pane_id) else {
                continue;
            };
            if rect.width == 0 || rect.height == 0 {
                continue;
            }
            let rendered = match pane.last_output.as_deref() {
                Some(raw) if !raw.is_empty() => render_terminal_styled(
                    raw,
                    rect.width,
                    rect.height,
                    if pane.active { scroll_offset } else { 0 },
                ),
                _ => RenderedPane {
                    lines: vec![pane_placeholder_line(pane)],
                    cursor: None,
                },
            };
            write_pane_region_at(
                &mut out,
                rect.y,
                rect.height,
                rect.x,
                rect.width,
                &rendered.lines,
                if pane.active { copy_mode } else { None },
            )?;
            if pane.active {
                if let Some((cy, cx)) = rendered.cursor {
                    active_cursor_screen = Some((rect.x + cx, rect.y + cy));
                }
                active_styled = rendered.lines;
            }
        }
    } else if active_window_layout == dmux_proto::SplitOrientation::Vertical {
        let requested: Vec<usize> = window_panes.iter().map(|p| p.cols as usize).collect();
        let widths = distribute_slots(&requested, cols_usize, 10);
        let mut col_cursor = 0usize;
        for (idx, pane) in window_panes.iter().enumerate() {
            let pane_width = widths.get(idx).copied().unwrap_or(0);
            if pane_width == 0 {
                continue;
            }
            let rendered = match pane.last_output.as_deref() {
                Some(raw) if !raw.is_empty() => render_terminal_styled(
                    raw,
                    pane_width,
                    content_height,
                    if pane.active { scroll_offset } else { 0 },
                ),
                _ => RenderedPane {
                    lines: vec![pane_placeholder_line(pane)],
                    cursor: None,
                },
            };
            write_pane_region_at(
                &mut out,
                content_start,
                content_height,
                col_cursor,
                pane_width,
                &rendered.lines,
                if pane.active { copy_mode } else { None },
            )?;
            if pane.active {
                if let Some((cy, cx)) = rendered.cursor {
                    active_cursor_screen = Some((col_cursor + cx, content_start + cy));
                }
                active_styled = rendered.lines;
            }
            col_cursor = col_cursor.saturating_add(pane_width);
        }
        if col_cursor < cols_usize {
            for row in content_start..(content_start + content_height) {
                render_fixed_line_at(&mut out, row, col_cursor, cols_usize - col_cursor, "")?;
            }
        }
    } else {
        let pane_count = window_panes.len().max(1);
        let header_rows = if pane_count > 1 { pane_count } else { 0 };
        let total_content = content_height.saturating_sub(header_rows).max(pane_count);
        let requested: Vec<usize> = window_panes.iter().map(|p| p.rows as usize).collect();
        let heights = distribute_slots(&requested, total_content, 1);
        let mut row_cursor = content_start;

        for (idx, pane) in window_panes.iter().enumerate() {
            let pane_height = heights.get(idx).copied().unwrap_or(0);
            if pane_height == 0 {
                continue;
            }

            if pane_count > 1 {
                let header = format!(
                    "── pane {}{} ({}) ─",
                    pane.pane_id,
                    if pane.active { "*" } else { "" },
                    pane.pane_title,
                );
                let (fg, bg) = if pane.active {
                    (Color::Black, Color::DarkGreen)
                } else {
                    (Color::White, Color::DarkGrey)
                };
                render_bar_line(&mut out, row_cursor, cols_usize, &header, fg, bg)?;
                row_cursor += 1;
            }

            let rendered = match pane.last_output.as_deref() {
                Some(raw) if !raw.is_empty() => render_terminal_styled(
                    raw,
                    cols_usize,
                    pane_height,
                    if pane.active { scroll_offset } else { 0 },
                ),
                _ => RenderedPane {
                    lines: vec![pane_placeholder_line(pane)],
                    cursor: None,
                },
            };

            write_pane_region_at(
                &mut out,
                row_cursor,
                pane_height,
                0,
                cols_usize,
                &rendered.lines,
                if pane.active { copy_mode } else { None },
            )?;
            if pane.active {
                if let Some((cy, cx)) = rendered.cursor {
                    active_cursor_screen = Some((cx, row_cursor + cy));
                }
                active_styled = rendered.lines;
            }
            row_cursor += pane_height;
        }
        while row_cursor < content_start + content_height {
            render_fixed_line(&mut out, row_cursor, cols_usize, "")?;
            row_cursor += 1;
        }
    }

    let floating_panes: Vec<&PaneInfo> = window_panes.iter().filter(|p| p.floating).copied().collect();
    for fp in &floating_panes {
        let raw = fp.last_output.as_deref().unwrap_or("");
        let rendered = render_terminal_styled(raw, cols_usize.saturating_sub(8), rows_usize.saturating_sub(6), 0);
        let title = format!("pane {} ({}) [floating]", fp.pane_id, fp.pane_title);
        let lines: Vec<String> = rendered.lines.iter().map(|line| line.iter().map(|c| c.ch).collect()).collect();
        render_popup_box(&mut out, cols_usize, rows_usize, &title, &lines)?;
    }
    if let Some(p) = popup {
        render_popup_box(&mut out, cols_usize, rows_usize, &p.title, &p.lines)?;
    }
    if let Some(input) = popup_input {
        let line = format!("popup-shell: {input}");
        let lines = vec![line];
        render_popup_box(&mut out, cols_usize, rows_usize, "run-shell", &lines)?;
    }

    if copy_mode.is_some() || popup.is_some() || popup_input.is_some() {
        execute!(out, cursor::Hide)?;
    } else if let Some((cx, cy)) = active_cursor_screen {
        execute!(out, cursor::MoveTo(cx as u16, cy as u16), cursor::Show)?;
    }
    out.write_all(b"\x1b[?2026l")?;
    out.flush()?;

    let frame_lines: Vec<String> = active_styled
        .iter()
        .map(|line| {
            line.iter()
                .map(|c| c.ch)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect();
    let frame_lines = if frame_lines.is_empty() {
        vec!["<shell ready: start typing in this window>".to_string()]
    } else {
        frame_lines
    };

    Ok(UiRenderFrame {
        content_lines: frame_lines,
    })
}

fn distribute_slots(requested: &[usize], total: usize, min_slot: usize) -> Vec<usize> {
    if requested.is_empty() {
        return vec![total];
    }
    let mut out = requested.to_vec();
    let claimed: usize = out.iter().filter(|&&v| v > 0).sum();
    let auto_count = out.iter().filter(|&&v| v == 0).count();
    let remaining = total.saturating_sub(claimed);
    let auto_share = if auto_count > 0 {
        remaining / auto_count
    } else {
        0
    };
    let mut auto_remainder = if auto_count > 0 {
        remaining - auto_share * auto_count
    } else {
        0
    };
    for slot in &mut out {
        if *slot == 0 {
            let extra = if auto_remainder > 0 {
                auto_remainder -= 1;
                1
            } else {
                0
            };
            *slot = auto_share + extra;
        }
    }
    let total_assigned: usize = out.iter().sum();
    if total_assigned > total && total_assigned > 0 {
        for slot in &mut out {
            *slot = (*slot * total) / total_assigned;
        }
    }
    for slot in &mut out {
        if *slot == 0 {
            *slot = min_slot.min(total.max(1));
        }
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct PaneRect {
    pane_id: u64,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

fn build_split_rects(
    node: &dmux_proto::SplitNodeInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    out: &mut Vec<PaneRect>,
) {
    if width == 0 || height == 0 {
        return;
    }
    match node {
        dmux_proto::SplitNodeInfo::Leaf { pane_id } => out.push(PaneRect {
            pane_id: *pane_id,
            x,
            y,
            width,
            height,
        }),
        dmux_proto::SplitNodeInfo::Split {
            orientation,
            split_ratio,
            first,
            second,
        } => match orientation {
            dmux_proto::SplitOrientation::Horizontal => {
                if height <= 2 {
                    build_split_rects(first, x, y, width, height, out);
                    return;
                }
                let axis = height - 1;
                let ratio = (*split_ratio).clamp(50, 950) as usize;
                let mut top = axis * ratio / 1000;
                top = top.clamp(1, axis.saturating_sub(1));
                let bottom = axis.saturating_sub(top);
                build_split_rects(first, x, y, width, top, out);
                build_split_rects(second, x, y + top + 1, width, bottom, out);
            }
            dmux_proto::SplitOrientation::Vertical => {
                if width <= 2 {
                    build_split_rects(first, x, y, width, height, out);
                    return;
                }
                let axis = width - 1;
                let ratio = (*split_ratio).clamp(50, 950) as usize;
                let mut left = axis * ratio / 1000;
                left = left.clamp(1, axis.saturating_sub(1));
                let right = axis.saturating_sub(left);
                build_split_rects(first, x, y, left, height, out);
                build_split_rects(second, x + left + 1, y, right, height, out);
            }
        },
    }
}

fn draw_split_borders(
    node: &dmux_proto::SplitNodeInfo,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    out: &mut std::io::Stdout,
) -> Result<()> {
    if width == 0 || height == 0 {
        return Ok(());
    }
    match node {
        dmux_proto::SplitNodeInfo::Leaf { .. } => Ok(()),
        dmux_proto::SplitNodeInfo::Split {
            orientation,
            split_ratio,
            first,
            second,
        } => match orientation {
            dmux_proto::SplitOrientation::Horizontal => {
                if height <= 2 {
                    return Ok(());
                }
                let axis = height - 1;
                let ratio = (*split_ratio).clamp(50, 950) as usize;
                let mut top = axis * ratio / 1000;
                top = top.clamp(1, axis.saturating_sub(1));
                let border_row = y + top;
                render_fixed_line_at(out, border_row, x, width, &"─".repeat(width))?;
                draw_split_borders(first, x, y, width, top, out)?;
                draw_split_borders(second, x, y + top + 1, width, axis - top, out)
            }
            dmux_proto::SplitOrientation::Vertical => {
                if width <= 2 {
                    return Ok(());
                }
                let axis = width - 1;
                let ratio = (*split_ratio).clamp(50, 950) as usize;
                let mut left = axis * ratio / 1000;
                left = left.clamp(1, axis.saturating_sub(1));
                let border_col = x + left;
                for row in y..(y + height) {
                    render_fixed_line_at(out, row, border_col, 1, "│")?;
                }
                draw_split_borders(first, x, y, left, height, out)?;
                draw_split_borders(second, x + left + 1, y, axis - left, height, out)
            }
        },
    }
}

fn write_pane_region_at(
    out: &mut std::io::Stdout,
    start_row: usize,
    height: usize,
    start_col: usize,
    cols: usize,
    styled: &[StyledLine],
    copy_mode: Option<&CopyModeState>,
) -> Result<()> {
    for idx in 0..height {
        let row = start_row + idx;
        if let Some(line) = styled.get(idx) {
            if let Some(copy) = copy_mode {
                let plain: String = line.iter().map(|c| c.ch).collect();
                let overlay = render_copy_overlay_line(&plain, idx, copy);
                render_fixed_line_at(out, row, start_col, cols, &overlay)?;
            } else {
                render_styled_line_at(out, row, start_col, cols, line)?;
            }
        } else {
            render_fixed_line_at(out, row, start_col, cols, "")?;
        }
    }
    Ok(())
}

fn render_styled_line_at(
    out: &mut std::io::Stdout,
    row: usize,
    start_col: usize,
    cols: usize,
    line: &[Cell],
) -> Result<()> {
    execute!(
        out,
        cursor::MoveTo(start_col as u16, row as u16),
        ResetColor
    )?;
    let mut current = CellStyle::default();
    let mut written = 0usize;
    for cell in line.iter().take(cols) {
        if cell.style != current {
            apply_style_diff(out, &current, &cell.style)?;
            current = cell.style;
        }
        let mut buf = [0u8; 4];
        out.write_all(cell.ch.encode_utf8(&mut buf).as_bytes())?;
        written += 1;
    }
    execute!(out, ResetColor)?;
    if written < cols {
        out.write_all(" ".repeat(cols - written).as_bytes())?;
    }
    Ok(())
}

fn apply_style_diff(out: &mut std::io::Stdout, _prev: &CellStyle, next: &CellStyle) -> Result<()> {
    execute!(out, ResetColor)?;
    if let Some(fg) = next.fg {
        execute!(out, SetForegroundColor(fg))?;
    }
    if let Some(bg) = next.bg {
        execute!(out, SetBackgroundColor(bg))?;
    }
    if next.bold {
        out.write_all(b"\x1b[1m")?;
    }
    if next.underline {
        out.write_all(b"\x1b[4m")?;
    }
    if next.reverse {
        out.write_all(b"\x1b[7m")?;
    }
    Ok(())
}

fn render_fixed_line(out: &mut std::io::Stdout, row: usize, cols: usize, text: &str) -> Result<()> {
    render_fixed_line_at(out, row, 0, cols, text)
}

fn render_fixed_line_at(
    out: &mut std::io::Stdout,
    row: usize,
    start_col: usize,
    cols: usize,
    text: &str,
) -> Result<()> {
    execute!(
        out,
        cursor::MoveTo(start_col as u16, row as u16),
        ResetColor
    )?;
    let clipped = clip_to_chars(text, cols);
    out.write_all(clipped.as_bytes())?;
    let used = clipped.chars().count();
    if used < cols {
        out.write_all(" ".repeat(cols - used).as_bytes())?;
    }
    Ok(())
}

fn render_popup_box(
    out: &mut std::io::Stdout,
    total_cols: usize,
    total_rows: usize,
    title: &str,
    lines: &[String],
) -> Result<()> {
    let content_w = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0).max(title.chars().count() + 4);
    let box_w = (content_w + 4).min(total_cols.saturating_sub(4)).max(20);
    let inner_w = box_w.saturating_sub(2);
    let box_h = (lines.len() + 4).min(total_rows.saturating_sub(4)).max(5);
    let inner_h = box_h.saturating_sub(2);
    let start_col = (total_cols.saturating_sub(box_w)) / 2;
    let start_row = (total_rows.saturating_sub(box_h)) / 2;
    let fg = Color::White;
    let bg = Color::DarkBlue;

    let title_line = format!("┌─ {title} ", );
    let title_full: String = title_line.chars().chain(std::iter::repeat('─')).take(inner_w).collect::<String>() + "┐";
    render_fixed_line_at_styled(out, start_row, start_col, box_w, &title_full, fg, bg)?;

    for i in 0..inner_h {
        let body = match lines.get(i) {
            Some(l) => {
                let mut s = String::from("│ ");
                let mut count = 2;
                for ch in l.chars() {
                    if count + 1 >= box_w.saturating_sub(1) { break; }
                    s.push(ch);
                    count += 1;
                }
                while count < box_w.saturating_sub(1) {
                    s.push(' ');
                    count += 1;
                }
                s.push('│');
                s
            }
            None => {
                let mut s = String::from("│");
                for _ in 0..box_w.saturating_sub(2) { s.push(' '); }
                s.push('│');
                s
            }
        };
        render_fixed_line_at_styled(out, start_row + 1 + i, start_col, box_w, &body, fg, bg)?;
    }
    let bottom: String = std::iter::once('└').chain(std::iter::repeat('─').take(box_w.saturating_sub(2))).chain(std::iter::once('┘')).collect();
    render_fixed_line_at_styled(out, start_row + box_h.saturating_sub(1), start_col, box_w, &bottom, fg, bg)?;
    Ok(())
}

fn render_fixed_line_at_styled(
    out: &mut std::io::Stdout,
    row: usize,
    start_col: usize,
    cols: usize,
    text: &str,
    fg: Color,
    bg: Color,
) -> Result<()> {
    execute!(
        out,
        cursor::MoveTo(start_col as u16, row as u16),
        SetForegroundColor(fg),
        SetBackgroundColor(bg)
    )?;
    let clipped: String = text.chars().take(cols).collect();
    out.write_all(clipped.as_bytes())?;
    let used = clipped.chars().count();
    if used < cols {
        out.write_all(" ".repeat(cols - used).as_bytes())?;
    }
    execute!(out, ResetColor)?;
    Ok(())
}

fn render_bar_line(
    out: &mut std::io::Stdout,
    row: usize,
    cols: usize,
    text: &str,
    fg: Color,
    bg: Color,
) -> Result<()> {
    execute!(
        out,
        cursor::MoveTo(0, row as u16),
        SetForegroundColor(fg),
        SetBackgroundColor(bg)
    )?;
    let clipped = clip_to_chars(text, cols);
    out.write_all(clipped.as_bytes())?;
    let used = clipped.chars().count();
    if used < cols {
        out.write_all(" ".repeat(cols - used).as_bytes())?;
    }
    execute!(out, ResetColor)?;
    Ok(())
}

fn clip_to_chars(input: &str, max_chars: usize) -> String {
    input.chars().take(max_chars).collect()
}

fn render_copy_overlay_line(line: &str, row_idx: usize, copy: &CopyModeState) -> String {
    let mut chars: Vec<char> = line.chars().collect();

    if let Some((a_row, a_col)) = copy.anchor {
        let ((start_row, start_col), (end_row, end_col)) =
            normalize_selection((a_row, a_col), (copy.cursor_row, copy.cursor_col));
        if row_idx >= start_row && row_idx <= end_row {
            let line_start = if row_idx == start_row { start_col } else { 0 };
            let line_end = if row_idx == end_row {
                end_col
            } else {
                chars.len().saturating_sub(1)
            };
            if !chars.is_empty() {
                for idx in line_start..=line_end.min(chars.len().saturating_sub(1)) {
                    if chars[idx] == ' ' {
                        chars[idx] = '·';
                    }
                }
            }
        }
    }

    if row_idx == copy.cursor_row {
        if chars.is_empty() {
            chars.push('█');
        } else {
            let col = copy.cursor_col.min(chars.len().saturating_sub(1));
            chars[col] = '█';
        }
    }

    chars.into_iter().collect()
}

fn normalize_selection(a: (usize, usize), b: (usize, usize)) -> ((usize, usize), (usize, usize)) {
    if a.0 < b.0 || (a.0 == b.0 && a.1 <= b.1) {
        (a, b)
    } else {
        (b, a)
    }
}

fn yank_selection(lines: &[String], copy: &CopyModeState) -> Option<String> {
    let anchor = copy.anchor?;
    let ((start_row, start_col), (end_row, end_col)) =
        normalize_selection(anchor, (copy.cursor_row, copy.cursor_col));

    if lines.is_empty() || start_row >= lines.len() {
        return None;
    }

    let mut out = String::new();
    for row in start_row..=end_row.min(lines.len().saturating_sub(1)) {
        let chars: Vec<char> = lines[row].chars().collect();
        if chars.is_empty() {
            if row != end_row {
                out.push('\n');
            }
            continue;
        }
        let from = if row == start_row { start_col } else { 0 };
        let to = if row == end_row {
            end_col
        } else {
            chars.len().saturating_sub(1)
        };
        let from = from.min(chars.len().saturating_sub(1));
        let to = to.min(chars.len().saturating_sub(1));
        for ch in &chars[from..=to] {
            out.push(*ch);
        }
        if row != end_row {
            out.push('\n');
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn build_meta_lines(snapshot: &UiSnapshot) -> Vec<String> {
    let mut lines = vec!["--- metadata ---".to_string()];
    lines.push("sessions:".to_string());
    for s in &snapshot.sessions {
        lines.push(format!(
            "  id={} name={} windows={} panes={}",
            s.id, s.name, s.windows, s.panes
        ));
    }
    if snapshot.sessions.is_empty() {
        lines.push("  <none>".to_string());
    }
    lines.push("windows:".to_string());
    for w in &snapshot.windows {
        lines.push(format!(
            "  {} {}:{} panes={}",
            if w.active { "*" } else { " " },
            w.session_name,
            w.window_name,
            w.pane_count
        ));
    }
    if snapshot.windows.is_empty() {
        lines.push("  <none>".to_string());
    }
    lines
}

fn build_window_pane_bar(snapshot: &UiSnapshot) -> String {
    let attached = snapshot.attached_session.as_deref();
    let mut windows = snapshot
        .windows
        .iter()
        .filter(|w| attached.map(|s| s == w.session_name).unwrap_or(true))
        .collect::<Vec<_>>();
    windows.sort_by_key(|w| {
        w.window_name
            .parse::<usize>()
            .unwrap_or(usize::MAX.saturating_sub(1))
    });

    let window_chunks = windows
        .iter()
        .map(|w| {
            if w.active {
                format!("[{}]", w.window_name)
            } else {
                w.window_name.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ");

    let active_window = windows
        .iter()
        .find(|w| w.active)
        .map(|w| (w.session_name.clone(), w.window_name.clone()));
    let pane_chunks = if let Some((session_name, window_name)) = active_window {
        let mut panes = snapshot
            .panes
            .iter()
            .filter(|p| p.session_name == session_name && p.window_name == window_name)
            .collect::<Vec<_>>();
        panes.sort_by_key(|p| p.pane_id);
        panes
            .iter()
            .map(|p| {
                if p.active {
                    format!("<{}>", p.pane_id)
                } else {
                    p.pane_id.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        String::new()
    };

    format!("windows: {} | panes: {}", window_chunks, pane_chunks)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CellStyle {
    fg: Option<Color>,
    bg: Option<Color>,
    bold: bool,
    underline: bool,
    reverse: bool,
}

impl CellStyle {
    const fn default_const() -> Self {
        Self {
            fg: None,
            bg: None,
            bold: false,
            underline: false,
            reverse: false,
        }
    }
}

impl Default for CellStyle {
    fn default() -> Self {
        Self::default_const()
    }
}

#[derive(Debug, Clone, Copy)]
struct Cell {
    ch: char,
    style: CellStyle,
}

impl Cell {
    fn blank() -> Self {
        Self {
            ch: ' ',
            style: CellStyle::default(),
        }
    }
}

type StyledLine = Vec<Cell>;

#[derive(Debug, Clone)]
struct ParsedTerminal {
    lines: Vec<StyledLine>,
    cursor_row: usize,
    cursor_col: usize,
    cursor_visible: bool,
}

#[derive(Debug, Clone)]
struct RenderedPane {
    lines: Vec<StyledLine>,
    cursor: Option<(usize, usize)>,
}

fn render_terminal_styled(
    raw: &str,
    width: usize,
    max_lines: usize,
    scroll_offset: usize,
) -> RenderedPane {
    let normalized = parse_terminal_snapshot(raw, width.max(20));
    let lines: Vec<StyledLine> = normalized.lines;

    if lines.is_empty() || lines.iter().all(|line| line.iter().all(|c| c.ch == ' ')) {
        return RenderedPane {
            lines: vec![styled_from_str("<starting shell...>")],
            cursor: None,
        };
    }

    let max_scroll = lines.len().saturating_sub(1);
    let offset = scroll_offset.min(max_scroll);
    let end = lines.len().saturating_sub(offset);
    let start = end.saturating_sub(max_lines);
    let cursor = if normalized.cursor_visible
        && normalized.cursor_row >= start
        && normalized.cursor_row < end
    {
        Some((
            normalized.cursor_row - start,
            normalized.cursor_col.min(width.saturating_sub(1)),
        ))
    } else {
        None
    };

    RenderedPane {
        lines: lines[start..end].to_vec(),
        cursor,
    }
}

fn pane_placeholder_line(pane: &PaneInfo) -> StyledLine {
    let no_input = pane.last_input.as_deref().map(|s| s.is_empty()).unwrap_or(true);
    let no_output = pane.last_output.as_deref().map(|s| s.is_empty()).unwrap_or(true);
    if no_input && no_output {
        styled_from_str("<starting shell...>")
    } else {
        styled_from_str("<empty>")
    }
}

fn styled_from_str(s: &str) -> StyledLine {
    s.chars()
        .map(|ch| Cell {
            ch,
            style: CellStyle::default(),
        })
        .collect()
}

fn parse_terminal_snapshot(raw: &str, width: usize) -> ParsedTerminal {
    let mut lines: Vec<StyledLine> = vec![Vec::new()];
    let mut row = 0usize;
    let mut col = 0usize;
    let mut saved = (0usize, 0usize);
    let mut style = CellStyle::default();
    let mut cursor_visible = true;
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;

    while i < chars.len() {
        let ch = chars[i];
        i += 1;
        match ch {
            '\u{1b}' => {
                if i >= chars.len() {
                    break;
                }
                match chars[i] {
                    '[' => {
                        i += 1;
                        // skip private markers like '?'
                        let mut private = false;
                        if i < chars.len()
                            && (chars[i] == '?' || chars[i] == '>' || chars[i] == '=')
                        {
                            private = true;
                            i += 1;
                        }
                        let start = i;
                        while i < chars.len() && !chars[i].is_ascii_alphabetic() && chars[i] != '~'
                        {
                            i += 1;
                        }
                        if i >= chars.len() {
                            break;
                        }
                        let cmd = chars[i];
                        let params_str: String = chars[start..i].iter().collect();
                        i += 1;
                        let params = parse_csi_params(&params_str);
                        if !private {
                            apply_csi(
                                cmd, &params, width, &mut lines, &mut row, &mut col, &mut saved,
                                &mut style,
                            );
                        } else {
                            apply_private_csi(cmd, &params, &mut cursor_visible);
                        }
                    }
                    ']' => {
                        i += 1;
                        while i < chars.len() {
                            if chars[i] == '\u{7}' {
                                i += 1;
                                break;
                            }
                            if chars[i] == '\u{1b}' && i + 1 < chars.len() && chars[i + 1] == '\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
            '\r' => col = 0,
            '\n' => {
                row += 1;
                ensure_row(&mut lines, row);
                col = 0;
            }
            '\u{8}' => col = col.saturating_sub(1),
            '\t' => {
                let next_tab = ((col / 8) + 1) * 8;
                while col < next_tab {
                    put_char(&mut lines, row, &mut col, ' ', width, style);
                }
            }
            c if c.is_control() => {}
            c => put_char(&mut lines, row, &mut col, c, width, style),
        }
    }

    ParsedTerminal {
        lines,
        cursor_row: row,
        cursor_col: col,
        cursor_visible,
    }
}

fn parse_csi_params(params: &str) -> Vec<usize> {
    params
        .split(';')
        .map(|part| {
            if part.is_empty() {
                0
            } else {
                part.parse::<usize>().unwrap_or(0)
            }
        })
        .collect()
}

fn apply_csi(
    cmd: char,
    params: &[usize],
    width: usize,
    lines: &mut Vec<StyledLine>,
    row: &mut usize,
    col: &mut usize,
    saved: &mut (usize, usize),
    style: &mut CellStyle,
) {
    let p0_default = |d: usize| {
        let v = params.first().copied().unwrap_or(0);
        if v == 0 {
            d
        } else {
            v
        }
    };
    match cmd {
        'A' => *row = row.saturating_sub(p0_default(1)),
        'B' => {
            *row += p0_default(1);
            ensure_row(lines, *row);
        }
        'C' => *col = (*col + p0_default(1)).min(width.saturating_sub(1)),
        'D' => *col = col.saturating_sub(p0_default(1)),
        'E' => {
            *row += p0_default(1);
            ensure_row(lines, *row);
            *col = 0;
        }
        'F' => {
            *row = row.saturating_sub(p0_default(1));
            *col = 0;
        }
        'G' => *col = p0_default(1).saturating_sub(1).min(width.saturating_sub(1)),
        'H' | 'f' => {
            let r = params
                .first()
                .copied()
                .unwrap_or(1)
                .max(1)
                .saturating_sub(1);
            let c = params.get(1).copied().unwrap_or(1).max(1).saturating_sub(1);
            *row = r;
            *col = c.min(width.saturating_sub(1));
            ensure_row(lines, *row);
        }
        'J' => {
            let mode = params.first().copied().unwrap_or(0);
            if mode == 2 || mode == 3 {
                lines.clear();
                lines.push(Vec::new());
                *row = 0;
                *col = 0;
            }
        }
        'K' => {
            ensure_row(lines, *row);
            let mode = params.first().copied().unwrap_or(0);
            let line = &mut lines[*row];
            match mode {
                0 => {
                    if *col < line.len() {
                        line.truncate(*col);
                    }
                }
                1 => {
                    let upto = (*col).min(line.len());
                    for cell in line.iter_mut().take(upto) {
                        cell.ch = ' ';
                        cell.style = *style;
                    }
                }
                2 => line.clear(),
                _ => {}
            }
        }
        's' => *saved = (*row, *col),
        'u' => {
            *row = saved.0;
            *col = saved.1;
            ensure_row(lines, *row);
        }
        'm' => apply_sgr(params, style),
        _ => {}
    }
}

fn apply_private_csi(cmd: char, params: &[usize], cursor_visible: &mut bool) {
    let mode = params.first().copied().unwrap_or(0);
    if mode == 25 {
        match cmd {
            'h' => *cursor_visible = true,
            'l' => *cursor_visible = false,
            _ => {}
        }
    }
}

fn apply_sgr(params: &[usize], style: &mut CellStyle) {
    let effective: &[usize] = if params.is_empty() { &[0] } else { params };
    let mut i = 0;
    while i < effective.len() {
        let p = effective[i];
        match p {
            0 => *style = CellStyle::default(),
            1 => style.bold = true,
            22 => style.bold = false,
            4 => style.underline = true,
            24 => style.underline = false,
            7 => style.reverse = true,
            27 => style.reverse = false,
            30..=37 => style.fg = Some(ansi_basic_color(p - 30, false)),
            39 => style.fg = None,
            40..=47 => style.bg = Some(ansi_basic_color(p - 40, false)),
            49 => style.bg = None,
            90..=97 => style.fg = Some(ansi_basic_color(p - 90, true)),
            100..=107 => style.bg = Some(ansi_basic_color(p - 100, true)),
            38 | 48 => {
                let target_fg = p == 38;
                if i + 1 < effective.len() {
                    match effective[i + 1] {
                        5 => {
                            if i + 2 < effective.len() {
                                let c = ansi_256_color(effective[i + 2] as u8);
                                if target_fg {
                                    style.fg = Some(c)
                                } else {
                                    style.bg = Some(c)
                                }
                                i += 2;
                            }
                        }
                        2 => {
                            if i + 4 < effective.len() {
                                let c = Color::Rgb {
                                    r: effective[i + 2] as u8,
                                    g: effective[i + 3] as u8,
                                    b: effective[i + 4] as u8,
                                };
                                if target_fg {
                                    style.fg = Some(c)
                                } else {
                                    style.bg = Some(c)
                                }
                                i += 4;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
}

fn ansi_basic_color(idx: usize, bright: bool) -> Color {
    match (idx, bright) {
        (0, false) => Color::Black,
        (1, false) => Color::DarkRed,
        (2, false) => Color::DarkGreen,
        (3, false) => Color::DarkYellow,
        (4, false) => Color::DarkBlue,
        (5, false) => Color::DarkMagenta,
        (6, false) => Color::DarkCyan,
        (7, false) => Color::Grey,
        (0, true) => Color::DarkGrey,
        (1, true) => Color::Red,
        (2, true) => Color::Green,
        (3, true) => Color::Yellow,
        (4, true) => Color::Blue,
        (5, true) => Color::Magenta,
        (6, true) => Color::Cyan,
        (7, true) => Color::White,
        _ => Color::Reset,
    }
}

fn ansi_256_color(n: u8) -> Color {
    match n {
        0..=7 => ansi_basic_color(n as usize, false),
        8..=15 => ansi_basic_color((n - 8) as usize, true),
        16..=231 => {
            let v = n - 16;
            let r = v / 36;
            let g = (v % 36) / 6;
            let b = v % 6;
            let scale = |c: u8| if c == 0 { 0u8 } else { 55 + c * 40 };
            Color::Rgb {
                r: scale(r),
                g: scale(g),
                b: scale(b),
            }
        }
        232..=255 => {
            let g = 8 + (n - 232) * 10;
            Color::Rgb { r: g, g, b: g }
        }
    }
}

fn ensure_row(lines: &mut Vec<StyledLine>, row: usize) {
    while lines.len() <= row {
        lines.push(Vec::new());
    }
}

fn put_char(
    lines: &mut Vec<StyledLine>,
    row: usize,
    col: &mut usize,
    ch: char,
    width: usize,
    style: CellStyle,
) {
    ensure_row(lines, row);
    let line = &mut lines[row];
    if *col >= width {
        return;
    }
    if line.len() <= *col {
        line.resize(*col + 1, Cell::blank());
    }
    line[*col] = Cell { ch, style };
    *col += 1;
}

fn handle_prefix_key(
    socket: &std::path::Path,
    snapshot: &UiSnapshot,
    key: KeyCode,
) -> Result<Option<String>> {
    let Some(session) = snapshot.attached_session.clone() else {
        return Ok(Some("no attached session".to_string()));
    };

    let notice = match key {
        KeyCode::Char('c') => {
            let resp = send_request(
                socket,
                Request::CreateWindow {
                    session: Some(session),
                    name: None,
                },
            )?;
            response_notice(&resp, "window created")
        }
        KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right => {
            let direction = match key {
                KeyCode::Up => dmux_proto::ResizeDir::Up,
                KeyCode::Down => dmux_proto::ResizeDir::Down,
                KeyCode::Left => dmux_proto::ResizeDir::Left,
                _ => dmux_proto::ResizeDir::Right,
            };
            let (window, pane) = {
                let win = snapshot
                    .windows
                    .iter()
                    .find(|w| w.active && w.session_name == session);
                let w = win.map(|w| w.window_name.clone());
                let p = win.and_then(|win| {
                    snapshot
                        .panes
                        .iter()
                        .find(|p| {
                            p.active
                                && p.session_name == session
                                && p.window_name == win.window_name
                        })
                        .map(|p| p.pane_id.to_string())
                });
                (w, p)
            };
            let resp = send_request(
                socket,
                Request::ResizePane {
                    session: Some(session),
                    window,
                    pane,
                    direction,
                    amount: 3,
                },
            )?;
            response_notice(&resp, "pane resized")
        }
        KeyCode::Char('"') | KeyCode::Char('%') => {
            let active_window = snapshot
                .windows
                .iter()
                .find(|w| w.active && w.session_name == session)
                .map(|w| w.window_name.clone());
            let orientation = if key == KeyCode::Char('%') {
                dmux_proto::SplitOrientation::Vertical
            } else {
                dmux_proto::SplitOrientation::Horizontal
            };
            let resp = send_request(
                socket,
                Request::SplitWindow {
                    session: Some(session),
                    window: active_window,
                    title: None,
                    orientation,
                },
            )?;
            response_notice(&resp, "pane split")
        }
        KeyCode::Char('n') => {
            let resp = send_request(
                socket,
                Request::NextWindow {
                    session: Some(session),
                },
            )?;
            response_notice(&resp, "next window")
        }
        KeyCode::Char('p') => {
            let resp = send_request(
                socket,
                Request::PreviousWindow {
                    session: Some(session),
                },
            )?;
            response_notice(&resp, "previous window")
        }
        KeyCode::Char('x') => {
            let win = snapshot
                .windows
                .iter()
                .find(|w| w.active && w.session_name == session);
            let active_window = win.map(|w| w.window_name.clone());
            let active_pane = win.and_then(|w| {
                snapshot
                    .panes
                    .iter()
                    .find(|p| {
                        p.active
                            && p.session_name == session
                            && p.window_name == w.window_name
                    })
                    .map(|p| p.pane_id.to_string())
            });
            let resp = send_request(
                socket,
                Request::KillPane {
                    session: Some(session),
                    window: active_window,
                    pane: active_pane,
                },
            )?;
            response_notice(&resp, "pane killed")
        }
        _ => None,
    };
    Ok(notice)
}

fn response_notice(resp: &Response, ok_msg: &str) -> Option<String> {
    if let Response::Error { message } = resp {
        Some(format!("error: {message}"))
    } else {
        Some(ok_msg.to_string())
    }
}

fn forward_key_to_active_pane(
    socket: &std::path::Path,
    snapshot: &UiSnapshot,
    key: KeyEvent,
) -> Result<()> {
    let Some(input) = key_event_to_literal(key) else {
        return Ok(());
    };
    let Some((session, window, pane)) = active_pane_target(snapshot) else {
        return Ok(());
    };

    let response = send_request(
        socket,
        Request::SendKeys {
            session: Some(session),
            window: Some(window),
            pane: Some(pane),
            keys: vec![input],
            literal: true,
        },
    )?;
    if let Response::Error { message } = response {
        bail!(message);
    }
    Ok(())
}

fn select_next_pane(socket: &std::path::Path, snapshot: &UiSnapshot) -> Result<()> {
    let Some((session_name, window_name, pane_id)) = active_pane_target(snapshot) else {
        return Ok(());
    };
    let pane_num = pane_id.parse::<u64>().unwrap_or(0);
    let mut pane_ids: Vec<u64> = snapshot
        .panes
        .iter()
        .filter(|p| p.session_name == session_name && p.window_name == window_name)
        .map(|p| p.pane_id)
        .collect();
    pane_ids.sort_unstable();
    if pane_ids.len() <= 1 {
        return Ok(());
    }

    let idx = pane_ids.iter().position(|id| *id == pane_num).unwrap_or(0);
    let next = pane_ids[(idx + 1) % pane_ids.len()].to_string();
    let _ = send_request(
        socket,
        Request::SelectPane {
            session: Some(session_name),
            window: Some(window_name),
            pane: next,
        },
    )?;
    Ok(())
}

fn handle_mouse(
    socket: &std::path::Path,
    snapshot: &UiSnapshot,
    me: MouseEvent,
    scroll_offset: &mut usize,
    scroll_mode: &mut bool,
) -> Result<()> {
    match me.kind {
        MouseEventKind::ScrollUp => {
            *scroll_mode = true;
            *scroll_offset = scroll_offset.saturating_add(3);
        }
        MouseEventKind::ScrollDown => {
            *scroll_offset = scroll_offset.saturating_sub(3);
            if *scroll_offset == 0 { *scroll_mode = false; }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            let Some((session, window)) = active_window_target(snapshot) else { return Ok(()); };
            let panes: Vec<&PaneInfo> = snapshot.panes.iter()
                .filter(|p| p.session_name == session && p.window_name == window)
                .collect();
            if panes.len() <= 1 { return Ok(()); }
            let row = me.row as usize;
            let content_start: usize = 1;
            let pane_count = panes.len();
            let header_rows = if pane_count > 1 { pane_count } else { 0 };
            let (term_cols, term_rows) = terminal::size().unwrap_or((120, 40));
            let _ = term_cols;
            let content_height = (term_rows as usize).saturating_sub(content_start + 2 + header_rows).max(pane_count);
            let auto = content_height / pane_count.max(1);
            let mut acc = content_start;
            let mut chosen: Option<u64> = None;
            for p in &panes {
                let h = 1 + auto;
                if row >= acc && row < acc + h {
                    chosen = Some(p.pane_id);
                    break;
                }
                acc += h;
            }
            if let Some(pid) = chosen {
                let _ = send_request(socket, Request::SelectPane {
                    session: Some(session),
                    window: Some(window),
                    pane: pid.to_string(),
                })?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn active_pane_target(snapshot: &UiSnapshot) -> Option<(String, String, String)> {
    let attached = snapshot.attached_session.as_deref();
    let active_window = snapshot
        .windows
        .iter()
        .find(|w| w.active && attached.map(|s| s == w.session_name).unwrap_or(true))
        .or_else(|| snapshot.windows.iter().find(|w| w.active))?;
    snapshot
        .panes
        .iter()
        .find(|p| {
            p.active
                && p.session_name == active_window.session_name
                && p.window_name == active_window.window_name
        })
        .map(|p| {
            (
                p.session_name.clone(),
                p.window_name.clone(),
                p.pane_id.to_string(),
            )
        })
}

fn active_window_target(snapshot: &UiSnapshot) -> Option<(String, String)> {
    let attached = snapshot.attached_session.as_deref();
    snapshot
        .windows
        .iter()
        .find(|w| w.active && attached.map(|s| s == w.session_name).unwrap_or(true))
        .or_else(|| snapshot.windows.iter().find(|w| w.active))
        .map(|w| (w.session_name.clone(), w.window_name.clone()))
}

fn sync_active_window_size(
    socket: &std::path::Path,
    snapshot: &UiSnapshot,
    cols: u16,
    rows: u16,
) -> Result<()> {
    let Some((session, window)) = active_window_target(snapshot) else {
        return Ok(());
    };
    let command = format!("resize-window -t {session}:{window} -x {cols} -y {rows}");
    let response = send_request(socket, Request::ExecuteRaw { command })?;
    if let Response::Error { message } = response {
        bail!(message);
    }
    Ok(())
}

fn key_event_to_literal(key: KeyEvent) -> Option<String> {
    let mut out = String::new();
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);

    if alt {
        out.push('\u{1b}');
    }

    match key.code {
        KeyCode::Char(c) => {
            if ctrl {
                if c.is_ascii_alphabetic() {
                    let upper = c.to_ascii_uppercase() as u8;
                    out.push((upper - b'@') as char);
                } else {
                    match c {
                        '[' => out.push('\u{1b}'),
                        '\\' => out.push('\u{1c}'),
                        ']' => out.push('\u{1d}'),
                        '^' => out.push('\u{1e}'),
                        '_' => out.push('\u{1f}'),
                        ' ' => out.push('\0'),
                        _ => return None,
                    }
                }
            } else {
                out.push(c);
            }
        }
        KeyCode::Enter => out.push('\n'),
        KeyCode::Tab => out.push('\t'),
        KeyCode::BackTab => out.push_str("\u{1b}[Z"),
        KeyCode::Backspace => out.push('\u{7f}'),
        KeyCode::Esc => out.push('\u{1b}'),
        KeyCode::Left => out.push_str("\u{1b}[D"),
        KeyCode::Right => out.push_str("\u{1b}[C"),
        KeyCode::Up => out.push_str("\u{1b}[A"),
        KeyCode::Down => out.push_str("\u{1b}[B"),
        KeyCode::Home => out.push_str("\u{1b}[H"),
        KeyCode::End => out.push_str("\u{1b}[F"),
        KeyCode::PageUp => out.push_str("\u{1b}[5~"),
        KeyCode::PageDown => out.push_str("\u{1b}[6~"),
        KeyCode::Delete => out.push_str("\u{1b}[3~"),
        KeyCode::Insert => out.push_str("\u{1b}[2~"),
        _ => return None,
    }

    Some(out)
}

fn print_response(response: Response) -> Result<()> {
    match response {
        Response::Pong => println!("pong"),
        Response::SessionCheck { name, exists } => {
            println!("session name={name} exists={exists}");
        }
        Response::SessionAttached { name } => {
            println!("attached session name={name}");
        }
        Response::ClientLocked { name } => {
            println!("locked client name={name}");
        }
        Response::ClientSuspended { name } => {
            println!("suspended client name={name}");
        }
        Response::ServerLocked => {
            println!("server locked");
        }
        Response::SessionLocked { name } => {
            println!("locked session name={name}");
        }
        Response::MessageDisplayed { message } => {
            println!("{message}");
        }
        Response::Messages(messages) => {
            if messages.is_empty() {
                println!("no messages");
            } else {
                for msg in messages {
                    println!("{msg}");
                }
            }
        }
        Response::OptionSet { name, value } => {
            println!("option {name}={value}");
        }
        Response::Options(options) => {
            if options.is_empty() {
                println!("no options");
            } else {
                for o in options {
                    println!("{}={}", o.name, o.value);
                }
            }
        }
        Response::EnvironmentSet { name, value } => {
            println!("env {name}={value}");
        }
        Response::Environment(envs) => {
            if envs.is_empty() {
                println!("no environment");
            } else {
                for e in envs {
                    println!("{}={}", e.name, e.value);
                }
            }
        }
        Response::ClientSwitched { from, to } => {
            println!(
                "switched client from={} to={}",
                from.unwrap_or_default(),
                to
            );
        }
        Response::ClientDetached { name } => {
            println!("detached session name={name}");
        }
        Response::SessionCreated { id, name } => {
            println!("created session id={id} name={name}");
        }
        Response::SessionKilled { id, name } => {
            println!("killed session id={id} name={name}");
        }
        Response::SessionRenamed {
            id,
            old_name,
            new_name,
        } => {
            println!("renamed session id={id} old={old_name} new={new_name}");
        }
        Response::WindowCreated { session, id, name } => {
            println!("created window id={id} name={name} session={session}");
        }
        Response::WindowRenamed {
            session,
            id,
            old_name,
            new_name,
        } => {
            println!("renamed window id={id} old={old_name} new={new_name} session={session}");
        }
        Response::WindowKilled { session, id, name } => {
            println!("killed window id={id} name={name} session={session}");
        }
        Response::WindowSelected { session, id, name } => {
            println!("selected window id={id} name={name} session={session}");
        }
        Response::PaneCreated {
            session,
            window_id,
            pane_id,
            title,
        } => {
            println!(
                "created pane id={pane_id} title={title} session={session} window_id={window_id}"
            );
        }
        Response::PaneKilled {
            session,
            window_id,
            pane_id,
            title,
        } => {
            println!(
                "killed pane id={pane_id} title={title} session={session} window_id={window_id}"
            );
        }
        Response::PaneSelected {
            session,
            window_id,
            pane_id,
            title,
        } => {
            println!(
                "selected pane id={pane_id} title={title} session={session} window_id={window_id}"
            );
        }
        Response::KeysSent {
            session,
            window_id,
            pane_id,
            payload,
        } => {
            println!(
                "sent keys to pane id={pane_id} session={session} window_id={window_id} payload={payload}"
            );
        }
        Response::Sessions(sessions) => {
            if sessions.is_empty() {
                println!("no sessions");
            } else {
                for s in sessions {
                    println!(
                        "id={} name={} windows={} panes={}",
                        s.id, s.name, s.windows, s.panes
                    );
                }
            }
        }
        Response::Commands(commands) => {
            if commands.is_empty() {
                println!("no commands");
            } else {
                for command in commands {
                    println!("{command}");
                }
            }
        }
        Response::Clients(clients) => {
            if clients.is_empty() {
                println!("no clients");
            } else {
                for c in clients {
                    println!(
                        "id={} name={} attached={} locked={} suspended={} server_locked={}",
                        c.id,
                        c.name,
                        c.attached_session.unwrap_or_default(),
                        c.locked,
                        c.suspended,
                        c.server_locked
                    );
                }
            }
        }
        Response::Windows(windows) => {
            if windows.is_empty() {
                println!("no windows");
            } else {
                for w in windows {
                    println!(
                        "session={}({}) window={}({}) panes={} active={} layout={:?}",
                        w.session_name,
                        w.session_id,
                        w.window_name,
                        w.window_id,
                        w.pane_count,
                        w.active,
                        w.layout
                    );
                }
            }
        }
        Response::Panes(panes) => {
            if panes.is_empty() {
                println!("no panes");
            } else {
                for p in panes {
                    let output_preview = p
                        .last_output
                        .as_deref()
                        .map(|s| {
                            let lines: Vec<String> = s
                                .lines()
                                .map(normalize_for_display)
                                .filter(|line| !line.trim().is_empty())
                                .collect();
                            if lines.is_empty() {
                                String::new()
                            } else if lines.len() == 1 {
                                lines[0].to_string()
                            } else {
                                format!("{} | {}", lines[lines.len() - 2], lines[lines.len() - 1])
                            }
                        })
                        .unwrap_or_default();
                    println!(
                        "session={}({}) window={}({}) pane={} title={} active={} last_input={} last_output={}",
                        p.session_name,
                        p.session_id,
                        p.window_name,
                        p.window_id,
                        p.pane_id,
                        p.pane_title,
                        p.active,
                        p.last_input.unwrap_or_default(),
                        output_preview
                    );
                }
            }
        }
        Response::CommandAccepted { command } => {
            println!("accepted raw command: {command}");
        }
        Response::PaneResized {
            session,
            window_id,
            pane_id,
            rows,
            cols,
        } => {
            println!("resized pane session={session} window_id={window_id} pane_id={pane_id} rows={rows} cols={cols}");
        }
        Response::PaneCaptured { pane_id, lines } => {
            println!("captured pane_id={pane_id} lines={}", lines.len());
            for l in lines {
                println!("{l}");
            }
        }
        Response::PaneFloated { pane_id, floating } => {
            println!("pane_id={pane_id} floating={floating}");
        }
        Response::Error { message } => {
            bail!(message);
        }
    }
    Ok(())
}

fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }

        match chars.next() {
            Some('[') => {
                while let Some(next) = chars.next() {
                    if next.is_ascii_alphabetic() || next == '~' {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        let _ = chars.next();
                        break;
                    }
                }
            }
            _ => {
                // Unsupported sequence form; discard introducer.
            }
        }
    }

    out
}

fn normalize_for_display(input: &str) -> String {
    let raw = strip_ansi(input).replace('\r', "\n");
    let filtered: String = raw
        .chars()
        .filter(|c| *c == '\n' || *c == '\t' || !c.is_control())
        .collect();

    let mut out = String::new();
    let mut empty_run = 0usize;
    for line in filtered.lines() {
        let trimmed_end = compact_visual_line(line);
        if trimmed_end.is_empty() {
            empty_run += 1;
            if empty_run > 1 {
                continue;
            }
            out.push('\n');
            continue;
        }
        empty_run = 0;
        out.push_str(&trimmed_end);
        out.push('\n');
    }

    out.trim_matches('\n').to_string()
}

fn compact_visual_line(line: &str) -> String {
    let trimmed_end = line.trim_end();
    let leading_ws = trimmed_end
        .chars()
        .take_while(|c| c.is_whitespace())
        .count();

    if leading_ws >= 24 {
        trimmed_end.trim_start().to_string()
    } else {
        trimmed_end.to_string()
    }
}
