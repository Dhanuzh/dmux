use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;
use std::{collections::BTreeMap, collections::HashSet};

use anyhow::{Context, Result};
use dmux_core::ServerState;
use dmux_proto::{ClientInfo, NameValue, PaneInfo, Request, Response, SessionInfo, WindowInfo};

#[derive(Debug, Default)]
struct CompatState {
    key_bindings: BTreeMap<String, String>,
    buffers: BTreeMap<String, String>,
    hooks: BTreeMap<String, String>,
    window_options: BTreeMap<String, String>,
    wait_tokens: HashSet<String>,
    prompt_history: Vec<String>,
}

static COMPAT_STATE: OnceLock<Mutex<CompatState>> = OnceLock::new();

fn compat_state() -> &'static Mutex<CompatState> {
    COMPAT_STATE.get_or_init(|| Mutex::new(CompatState::default()))
}

const MAX_PANE_OUTPUT_BYTES: usize = 256 * 1024;

struct PaneRuntime {
    master: Arc<Mutex<fs::File>>,
    output: Arc<Mutex<Vec<u8>>>,
    child: Child,
    pipe: Arc<Mutex<Option<PipeSink>>>,
}

struct PipeSink {
    stdin: std::process::ChildStdin,
    child: Child,
    cmd: String,
}

#[derive(Default)]
struct PtyRuntime {
    panes: BTreeMap<u64, PaneRuntime>,
}

impl PtyRuntime {
    fn ensure_pane(&mut self, pane_id: u64) -> Result<()> {
        if let Some(runtime) = self.panes.get_mut(&pane_id) {
            let alive = runtime
                .child
                .try_wait()
                .context("failed to check pane child process")?
                .is_none();
            if alive {
                return Ok(());
            }
        }

        if self.panes.contains_key(&pane_id) {
            self.remove_pane(pane_id);
        }

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let (master, slave) = open_pty_pair()?;

        let stdin = slave
            .try_clone()
            .context("failed to clone PTY slave for stdin")?;
        let stdout = slave
            .try_clone()
            .context("failed to clone PTY slave for stdout")?;
        let stderr = slave
            .try_clone()
            .context("failed to clone PTY slave for stderr")?;
        let slave_fd = slave.as_raw_fd();

        let shell_name = Path::new(&shell)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let mut command = ProcessCommand::new(&shell);
        match shell_name {
            "bash" | "zsh" | "sh" | "ksh" | "dash" | "fish" | "nu" => {
                command.arg("-i");
            }
            _ => {}
        }
        command
            .stdin(Stdio::from(stdin))
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .env("DMUX_PANE_ID", pane_id.to_string())
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor");

        unsafe {
            let ws = libc::winsize {
                ws_row: 40,
                ws_col: 120,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            libc::ioctl(slave_fd, libc::TIOCSWINSZ, &ws);
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(slave_fd, libc::TIOCSCTTY, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = command
            .spawn()
            .with_context(|| format!("failed to spawn shell for pane {pane_id}"))?;

        drop(slave);

        let output = Arc::new(Mutex::new(Vec::new()));
        let pipe: Arc<Mutex<Option<PipeSink>>> = Arc::new(Mutex::new(None));
        let reader = master.try_clone().context("failed to clone PTY master")?;
        spawn_output_reader(reader, Arc::clone(&output), Arc::clone(&pipe));

        self.panes.insert(
            pane_id,
            PaneRuntime {
                master: Arc::new(Mutex::new(master)),
                output,
                child,
                pipe,
            },
        );

        Ok(())
    }

    fn toggle_pipe(&self, pane_id: u64, cmd: Option<String>) -> Result<Option<String>> {
        let Some(runtime) = self.panes.get(&pane_id) else {
            return Ok(None);
        };
        let mut slot = runtime.pipe.lock().expect("pane pipe mutex poisoned");
        if slot.is_some() {
            if let Some(mut sink) = slot.take() {
                drop(sink.stdin);
                let _ = sink.child.kill();
                let _ = sink.child.wait();
                if cmd.is_none() {
                    return Ok(Some(format!("pipe-pane stopped ({})", sink.cmd)));
                }
            }
        }
        if let Some(cmd) = cmd {
            let mut child = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .with_context(|| format!("pipe-pane spawn: {cmd}"))?;
            let stdin = child.stdin.take().context("pipe-pane stdin missing")?;
            *slot = Some(PipeSink { stdin, child, cmd: cmd.clone() });
            return Ok(Some(format!("pipe-pane started: {cmd}")));
        }
        Ok(Some("pipe-pane already off".to_string()))
    }

    fn send_keys(&mut self, pane_id: u64, keys: &[String], literal: bool) -> Result<()> {
        self.ensure_pane(pane_id)?;
        let Some(runtime) = self.panes.get(&pane_id) else {
            return Ok(());
        };

        let payload = keys_to_bytes(keys, literal);
        let mut master = runtime.master.lock().expect("pane master mutex poisoned");
        master
            .write_all(&payload)
            .with_context(|| format!("failed writing keys to pane {pane_id}"))?;
        master
            .flush()
            .with_context(|| format!("failed flushing keys to pane {pane_id}"))?;

        Ok(())
    }

    fn output_for_pane(&self, pane_id: u64) -> Option<String> {
        self.panes.get(&pane_id).map(|runtime| {
            let output = runtime.output.lock().expect("pane output mutex poisoned");
            String::from_utf8_lossy(&output).to_string()
        })
    }

    fn output_snapshot(&self) -> Vec<(u64, String)> {
        self.panes
            .iter()
            .map(|(pane_id, runtime)| {
                let output = runtime.output.lock().expect("pane output mutex poisoned");
                (*pane_id, String::from_utf8_lossy(&output).to_string())
            })
            .collect()
    }

    fn resize_pty(&self, pane_id: u64, rows: u16, cols: u16) -> Result<()> {
        let Some(runtime) = self.panes.get(&pane_id) else {
            return Ok(());
        };
        let master = runtime.master.lock().expect("pane master mutex poisoned");
        let fd = master.as_raw_fd();
        unsafe {
            let ws = libc::winsize {
                ws_row: rows.max(1),
                ws_col: cols.max(1),
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            if libc::ioctl(fd, libc::TIOCSWINSZ, &ws) == -1 {
                return Err(io::Error::last_os_error()).context("TIOCSWINSZ failed");
            }
        }
        Ok(())
    }

    fn clear_history(&self, pane_id: u64) {
        if let Some(runtime) = self.panes.get(&pane_id) {
            runtime
                .output
                .lock()
                .expect("pane output mutex poisoned")
                .clear();
        }
    }

    fn respawn_pane(&mut self, pane_id: u64) -> Result<()> {
        if self.panes.contains_key(&pane_id) {
            self.remove_pane(pane_id);
        }
        self.ensure_pane(pane_id)
    }

    fn remove_pane(&mut self, pane_id: u64) {
        if let Some(mut runtime) = self.panes.remove(&pane_id) {
            let _ = runtime.child.kill();
            let _ = runtime.child.wait();
        }
    }

    fn remove_many(&mut self, pane_ids: impl IntoIterator<Item = u64>) {
        for pane_id in pane_ids {
            self.remove_pane(pane_id);
        }
    }
}

fn spawn_output_reader<R: Read + Send + 'static>(
    mut reader: R,
    output: Arc<Mutex<Vec<u8>>>,
    pipe: Arc<Mutex<Option<PipeSink>>>,
) {
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(read_n) => {
                    let slice = &buf[..read_n];
                    {
                        let mut out = output.lock().expect("pane output mutex poisoned");
                        out.extend_from_slice(slice);
                        if out.len() > MAX_PANE_OUTPUT_BYTES {
                            let overflow = out.len() - MAX_PANE_OUTPUT_BYTES;
                            out.drain(0..overflow);
                        }
                    }
                    let mut pipe_slot = pipe.lock().expect("pane pipe mutex poisoned");
                    let mut close_pipe = false;
                    if let Some(sink) = pipe_slot.as_mut() {
                        if sink.stdin.write_all(slice).is_err() {
                            close_pipe = true;
                        }
                    }
                    if close_pipe {
                        if let Some(mut sink) = pipe_slot.take() {
                            drop(sink.stdin);
                            let _ = sink.child.kill();
                            let _ = sink.child.wait();
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });
}

fn open_pty_pair() -> Result<(fs::File, fs::File)> {
    let master_fd = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if master_fd < 0 {
        return Err(io::Error::last_os_error()).context("posix_openpt failed");
    }

    let setup_result = (|| -> Result<(fs::File, fs::File)> {
        if unsafe { libc::grantpt(master_fd) } != 0 {
            return Err(io::Error::last_os_error()).context("grantpt failed");
        }
        if unsafe { libc::unlockpt(master_fd) } != 0 {
            return Err(io::Error::last_os_error()).context("unlockpt failed");
        }

        let mut name_buf = vec![0_i8; 256];
        if unsafe { libc::ptsname_r(master_fd, name_buf.as_mut_ptr(), name_buf.len()) } != 0 {
            return Err(io::Error::last_os_error()).context("ptsname_r failed");
        }

        let slave_fd = unsafe {
            libc::open(
                name_buf.as_ptr(),
                libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
            )
        };
        if slave_fd < 0 {
            return Err(io::Error::last_os_error()).context("open slave PTY failed");
        }

        let master = unsafe { fs::File::from_raw_fd(master_fd) };
        let slave = unsafe { fs::File::from_raw_fd(slave_fd) };
        Ok((master, slave))
    })();

    if setup_result.is_err() {
        let _ = unsafe { libc::close(master_fd) };
    }
    setup_result
}

fn keys_to_bytes(keys: &[String], literal: bool) -> Vec<u8> {
    if literal {
        return keys.join("").into_bytes();
    }

    let mut out = Vec::new();
    for (i, key) in keys.iter().enumerate() {
        let lower = key.to_ascii_lowercase();
        match lower.as_str() {
            "enter" | "c-m" => out.push(b'\n'),
            "space" => out.push(b' '),
            "tab" | "c-i" => out.push(b'\t'),
            "bspace" | "backspace" => out.push(0x7f),
            _ if lower.starts_with("c-") && key.len() == 3 => {
                if let Some(ctrl) = key.chars().nth(2) {
                    let upper = ctrl.to_ascii_uppercase() as u8;
                    if upper.is_ascii_alphabetic() {
                        out.push(upper - b'@');
                    }
                }
            }
            _ => {
                out.extend_from_slice(key.as_bytes());
                if i + 1 < keys.len() {
                    let next = keys[i + 1].to_ascii_lowercase();
                    if next != "enter" && next != "c-m" {
                        out.push(b' ');
                    }
                }
            }
        }
    }
    out
}

pub fn run(socket_path: &Path) -> Result<()> {
    if socket_path.exists() {
        fs::remove_file(socket_path).with_context(|| {
            format!("failed to remove stale socket at {}", socket_path.display())
        })?;
    }

    let listener = UnixListener::bind(socket_path)
        .with_context(|| format!("failed to bind socket {}", socket_path.display()))?;

    let runtime = Arc::new(Mutex::new(PtyRuntime::default()));
    let state = match load_persisted_state() {
        Some(s) => {
            let pane_ids = s.pane_ids();
            let arc = Arc::new(Mutex::new(s));
            let mut rt = runtime.lock().unwrap();
            for id in pane_ids {
                let _ = rt.ensure_pane(id);
            }
            arc
        }
        None => Arc::new(Mutex::new(ServerState::new())),
    };

    for stream in listener.incoming() {
        let stream = stream.context("failed to accept client connection")?;
        let state = Arc::clone(&state);
        let runtime = Arc::clone(&runtime);
        thread::spawn(move || {
            if let Err(error) = handle_client(stream, state, runtime) {
                eprintln!("dmux server: client handling error: {error}");
            }
        });
    }

    Ok(())
}

fn handle_client(
    mut stream: UnixStream,
    state: Arc<Mutex<ServerState>>,
    runtime: Arc<Mutex<PtyRuntime>>,
) -> Result<()> {
    let mut line = String::new();
    {
        let mut reader = BufReader::new(&mut stream);
        let bytes = reader
            .read_line(&mut line)
            .context("failed to read request line")?;
        if bytes == 0 {
            return Ok(());
        }
    }

    let response = match serde_json::from_str::<Request>(line.trim_end()) {
        Ok(request) => {
            let mutates = is_mutating(&request);
            let r = handle_request(request, Arc::clone(&state), Arc::clone(&runtime));
            if mutates {
                let snapshot = state.lock().unwrap();
                persist_state(&snapshot);
            }
            r
        }
        Err(error) => Response::Error {
            message: format!("invalid request JSON: {error}"),
        },
    };

    let wire = serde_json::to_string(&response).context("failed to encode response")?;
    stream
        .write_all(wire.as_bytes())
        .context("failed to write response")?;
    stream
        .write_all(b"\n")
        .context("failed to write trailing newline")?;
    stream.flush().context("failed to flush response")?;

    Ok(())
}

fn state_file_path() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/state")))?;
    let dir = base.join("dmux");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("state.json"))
}

fn persist_state(state: &ServerState) {
    let Some(path) = state_file_path() else { return };
    if let Ok(json) = state.to_json() {
        let _ = std::fs::write(&path, json);
    }
}

fn load_persisted_state() -> Option<ServerState> {
    let path = state_file_path()?;
    let contents = std::fs::read_to_string(&path).ok()?;
    ServerState::from_json(&contents).ok()
}

fn is_mutating(req: &Request) -> bool {
    !matches!(req,
        Request::Ping
        | Request::HasSession { .. }
        | Request::ListSessions
        | Request::ListCommands
        | Request::ListClients
        | Request::ListWindows { .. }
        | Request::ListPanes { .. }
        | Request::ShowMessages
        | Request::ShowOptions { .. }
        | Request::ShowEnvironment { .. }
        | Request::CapturePane { .. }
    )
}

fn handle_request(
    request: Request,
    state: Arc<Mutex<ServerState>>,
    runtime: Arc<Mutex<PtyRuntime>>,
) -> Response {
    match request {
        Request::Ping => Response::Pong,
        Request::HasSession { name } => {
            let state = state.lock().expect("state mutex poisoned");
            Response::SessionCheck {
                name: name.clone(),
                exists: state.has_session(&name),
            }
        }
        Request::AttachSession { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.attach_session(session.as_deref()) {
                Ok(session) => Response::SessionAttached { name: session.name },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::SwitchClient { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.switch_client(session.as_deref()) {
                Ok((from, to)) => Response::ClientSwitched { from, to: to.name },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::DetachClient { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.detach_client(session.as_deref()) {
                Ok(name) => Response::ClientDetached { name },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::LockClient => {
            let mut state = state.lock().expect("state mutex poisoned");
            let client = state.lock_client();
            Response::ClientLocked { name: client.name }
        }
        Request::SuspendClient => {
            let mut state = state.lock().expect("state mutex poisoned");
            let client = state.suspend_client();
            Response::ClientSuspended { name: client.name }
        }
        Request::LockServer => {
            let mut state = state.lock().expect("state mutex poisoned");
            let _ = state.lock_server();
            Response::ServerLocked
        }
        Request::LockSession { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.lock_session(session.as_deref()) {
                Ok(name) => Response::SessionLocked { name },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::DisplayMessage { message } => {
            let mut state = state.lock().expect("state mutex poisoned");
            Response::MessageDisplayed {
                message: state.display_message(message),
            }
        }
        Request::ShowMessages => {
            let state = state.lock().expect("state mutex poisoned");
            Response::Messages(state.show_messages())
        }
        Request::SetOption { name, value } => {
            let mut state = state.lock().expect("state mutex poisoned");
            let (name, value) = state.set_option(&name, &value);
            Response::OptionSet { name, value }
        }
        Request::ShowOptions { name } => {
            let state = state.lock().expect("state mutex poisoned");
            Response::Options(
                state
                    .show_options(name.as_deref())
                    .into_iter()
                    .map(|(name, value)| NameValue { name, value })
                    .collect(),
            )
        }
        Request::SetEnvironment { name, value } => {
            let mut state = state.lock().expect("state mutex poisoned");
            let (name, value) = state.set_environment(&name, &value);
            Response::EnvironmentSet { name, value }
        }
        Request::ShowEnvironment { name } => {
            let state = state.lock().expect("state mutex poisoned");
            Response::Environment(
                state
                    .show_environment(name.as_deref())
                    .into_iter()
                    .map(|(name, value)| NameValue { name, value })
                    .collect(),
            )
        }
        Request::CreateSession { name } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.create_session(name) {
                Ok(session) => {
                    if let Some(pane_id) = session
                        .windows
                        .first()
                        .and_then(|window| window.panes.first())
                        .map(|pane| pane.id)
                    {
                        let mut runtime = runtime.lock().expect("pty runtime mutex poisoned");
                        if let Err(error) = runtime.ensure_pane(pane_id) {
                            return Response::Error {
                                message: error.to_string(),
                            };
                        }
                    }
                    Response::SessionCreated {
                        id: session.id,
                        name: session.name,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::KillSession { name } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.kill_session(&name) {
                Ok(session) => {
                    let pane_ids = session
                        .windows
                        .iter()
                        .flat_map(|window| window.panes.iter().map(|pane| pane.id))
                        .collect::<Vec<_>>();
                    runtime
                        .lock()
                        .expect("pty runtime mutex poisoned")
                        .remove_many(pane_ids);
                    Response::SessionKilled {
                        id: session.id,
                        name: session.name,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::RenameSession { name, new_name } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.rename_session(&name, &new_name) {
                Ok(session) => Response::SessionRenamed {
                    id: session.id,
                    old_name: name,
                    new_name: session.name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::CreateWindow { session, name } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.create_window(session.as_deref(), name) {
                Ok((session, window)) => {
                    if let Some(pane_id) = window.panes.first().map(|pane| pane.id) {
                        let mut runtime = runtime.lock().expect("pty runtime mutex poisoned");
                        if let Err(error) = runtime.ensure_pane(pane_id) {
                            return Response::Error {
                                message: error.to_string(),
                            };
                        }
                    }
                    Response::WindowCreated {
                        session: session.name,
                        id: window.id,
                        name: window.name,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::RenameWindow {
            session,
            window,
            new_name,
        } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.rename_window(session.as_deref(), window.as_deref(), &new_name) {
                Ok((session, window, old_name)) => Response::WindowRenamed {
                    session: session.name,
                    id: window.id,
                    old_name,
                    new_name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::KillWindow { session, window } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.kill_window(session.as_deref(), window.as_deref()) {
                Ok((session, window)) => {
                    let pane_ids = window.panes.iter().map(|pane| pane.id).collect::<Vec<_>>();
                    runtime
                        .lock()
                        .expect("pty runtime mutex poisoned")
                        .remove_many(pane_ids);
                    Response::WindowKilled {
                        session: session.name,
                        id: window.id,
                        name: window.name,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::NextWindow { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_next_window(session.as_deref()) {
                Ok((session, window)) => Response::WindowSelected {
                    session: session.name,
                    id: window.id,
                    name: window.name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::PreviousWindow { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_previous_window(session.as_deref()) {
                Ok((session, window)) => Response::WindowSelected {
                    session: session.name,
                    id: window.id,
                    name: window.name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::LastWindow { session } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_last_window(session.as_deref()) {
                Ok((session, window)) => Response::WindowSelected {
                    session: session.name,
                    id: window.id,
                    name: window.name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::SelectWindow { session, window } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_window(session.as_deref(), &window) {
                Ok((session, window)) => Response::WindowSelected {
                    session: session.name,
                    id: window.id,
                    name: window.name,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::LastPane { session, window } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_last_pane(session.as_deref(), window.as_deref()) {
                Ok((session, window, pane)) => Response::PaneSelected {
                    session: session.name,
                    window_id: window.id,
                    pane_id: pane.id,
                    title: pane.title,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::SplitWindow {
            session,
            window,
            title,
            orientation,
        } => {
            let mut state = state.lock().expect("state mutex poisoned");
            let core_layout = match orientation {
                dmux_proto::SplitOrientation::Horizontal => dmux_core::WindowLayout::Horizontal,
                dmux_proto::SplitOrientation::Vertical => dmux_core::WindowLayout::Vertical,
            };
            match state.split_window(session.as_deref(), window.as_deref(), title, core_layout) {
                Ok((session, window, pane)) => {
                    let mut runtime = runtime.lock().expect("pty runtime mutex poisoned");
                    if let Err(error) = runtime.ensure_pane(pane.id) {
                        return Response::Error {
                            message: error.to_string(),
                        };
                    }
                    Response::PaneCreated {
                        session: session.name,
                        window_id: window.id,
                        pane_id: pane.id,
                        title: pane.title,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::KillPane {
            session,
            window,
            pane,
        } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.kill_pane(session.as_deref(), window.as_deref(), pane.as_deref()) {
                Ok((session, window, pane)) => {
                    runtime
                        .lock()
                        .expect("pty runtime mutex poisoned")
                        .remove_pane(pane.id);
                    Response::PaneKilled {
                        session: session.name,
                        window_id: window.id,
                        pane_id: pane.id,
                        title: pane.title,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::SelectPane {
            session,
            window,
            pane,
        } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.select_pane(session.as_deref(), window.as_deref(), &pane) {
                Ok((session, window, pane)) => Response::PaneSelected {
                    session: session.name,
                    window_id: window.id,
                    pane_id: pane.id,
                    title: pane.title,
                },
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::SendKeys {
            session,
            window,
            pane,
            keys,
            literal,
        } => {
            let mut state = state.lock().expect("state mutex poisoned");
            match state.send_keys(
                session.as_deref(),
                window.as_deref(),
                pane.as_deref(),
                &keys,
                literal,
                None,
            ) {
                Ok((session, window, pane, payload)) => {
                    {
                        let mut runtime = runtime.lock().expect("pty runtime mutex poisoned");
                        if let Err(error) = runtime.send_keys(pane.id, &keys, literal) {
                            return Response::Error {
                                message: error.to_string(),
                            };
                        }
                    }

                    thread::sleep(Duration::from_millis(15));
                    if let Some(output) = runtime
                        .lock()
                        .expect("pty runtime mutex poisoned")
                        .output_for_pane(pane.id)
                    {
                        let _ = state.set_pane_output(pane.id, output);
                    }

                    Response::KeysSent {
                        session: session.name,
                        window_id: window.id,
                        pane_id: pane.id,
                        payload,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ListSessions => {
            let state = state.lock().expect("state mutex poisoned");
            let sessions = state
                .sessions()
                .iter()
                .map(|session| SessionInfo {
                    id: session.id,
                    name: session.name.clone(),
                    windows: session.windows.len(),
                    panes: session.windows.iter().map(|w| w.panes.len()).sum(),
                })
                .collect();
            Response::Sessions(sessions)
        }
        Request::KillServer => {
            let mut state = state.lock().expect("state mutex poisoned");
            let pane_ids = state
                .sessions()
                .iter()
                .flat_map(|session| {
                    session
                        .windows
                        .iter()
                        .flat_map(|window| window.panes.iter().map(|pane| pane.id))
                })
                .collect::<Vec<_>>();
            runtime
                .lock()
                .expect("pty runtime mutex poisoned")
                .remove_many(pane_ids);
            let removed = state.kill_server();
            Response::CommandAccepted {
                command: format!("kill-server cleared_sessions={removed}"),
            }
        }
        Request::StartServer => {
            let mut state = state.lock().expect("state mutex poisoned");
            let _ = state.start_server();
            Response::CommandAccepted {
                command: "start-server ok".to_string(),
            }
        }
        Request::ListCommands => Response::Commands(vec![
            "attach-session".to_string(),
            "switch-client".to_string(),
            "detach-client".to_string(),
            "list-clients".to_string(),
            "lock-client".to_string(),
            "lock-server".to_string(),
            "lock-session".to_string(),
            "suspend-client".to_string(),
            "display-message".to_string(),
            "show-messages".to_string(),
            "set-option".to_string(),
            "show-options".to_string(),
            "set-environment".to_string(),
            "show-environment".to_string(),
            "kill-server".to_string(),
            "start-server".to_string(),
            "new-session".to_string(),
            "kill-session".to_string(),
            "has-session".to_string(),
            "rename-session".to_string(),
            "new-window".to_string(),
            "rename-window".to_string(),
            "kill-window".to_string(),
            "next-window".to_string(),
            "previous-window".to_string(),
            "last-window".to_string(),
            "select-window".to_string(),
            "split-window".to_string(),
            "kill-pane".to_string(),
            "select-pane".to_string(),
            "last-pane".to_string(),
            "send-keys".to_string(),
            "list-sessions".to_string(),
            "list-commands".to_string(),
            "list-windows".to_string(),
            "list-panes".to_string(),
        ]),
        Request::ListClients => {
            let state = state.lock().expect("state mutex poisoned");
            Response::Clients(
                state
                    .list_clients()
                    .into_iter()
                    .map(|c| ClientInfo {
                        id: c.id,
                        name: c.name,
                        attached_session: c.attached_session,
                        locked: c.locked,
                        suspended: c.suspended,
                        server_locked: c.server_locked,
                    })
                    .collect(),
            )
        }
        Request::ListWindows { session } => {
            let state = state.lock().expect("state mutex poisoned");
            match state.list_windows(session.as_deref()) {
                Ok(windows) => Response::Windows(
                    windows
                        .into_iter()
                        .map(|w| WindowInfo {
                            session_id: w.session_id,
                            session_name: w.session_name,
                            window_id: w.window_id,
                            window_name: w.window_name,
                            pane_count: w.pane_count,
                            active: w.active,
                            layout: match w.layout {
                                dmux_core::WindowLayout::Horizontal => {
                                    dmux_proto::SplitOrientation::Horizontal
                                }
                                dmux_core::WindowLayout::Vertical => {
                                    dmux_proto::SplitOrientation::Vertical
                                }
                            },
                            layout_tree: w.layout_tree.map(core_split_to_proto),
                        })
                        .collect(),
                ),
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ListPanes { session, window } => {
            let mut state = state.lock().expect("state mutex poisoned");
            let outputs = runtime
                .lock()
                .expect("pty runtime mutex poisoned")
                .output_snapshot();
            for (pane_id, output) in outputs {
                let _ = state.set_pane_output(pane_id, output);
            }
            match state.list_panes(session.as_deref(), window.as_deref()) {
                Ok(panes) => Response::Panes(
                    panes
                        .into_iter()
                        .map(|p| PaneInfo {
                            session_id: p.session_id,
                            session_name: p.session_name,
                            window_id: p.window_id,
                            window_name: p.window_name,
                            pane_id: p.pane_id,
                            pane_title: p.pane_title,
                            active: p.active,
                            last_input: p.last_input,
                            last_output: p.last_output,
                            rows: p.rows,
                            cols: p.cols,
                            floating: p.floating,
                        })
                        .collect(),
                ),
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ResizePane {
            session,
            window,
            pane,
            direction,
            amount,
        } => {
            let core_dir = match direction {
                dmux_proto::ResizeDir::Up => dmux_core::ResizeDirection::Up,
                dmux_proto::ResizeDir::Down => dmux_core::ResizeDirection::Down,
                dmux_proto::ResizeDir::Left => dmux_core::ResizeDirection::Left,
                dmux_proto::ResizeDir::Right => dmux_core::ResizeDirection::Right,
            };
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.resize_pane(
                session.as_deref(),
                window.as_deref(),
                pane.as_deref(),
                core_dir,
                amount,
            ) {
                Ok((session, window, pane)) => {
                    let rows = pane.rows;
                    let cols = pane.cols;
                    drop(state_guard);
                    if rows > 0 && cols > 0 {
                        let runtime_guard = runtime.lock().expect("pty runtime mutex poisoned");
                        let _ = runtime_guard.resize_pty(pane.id, rows, cols);
                    }
                    Response::PaneResized {
                        session: session.name,
                        window_id: window.id,
                        pane_id: pane.id,
                        rows,
                        cols,
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::CapturePane {
            session,
            window,
            pane,
            lines,
        } => {
            let mut state_guard = state.lock().expect("state mutex poisoned");
            let outputs = runtime
                .lock()
                .expect("pty runtime mutex poisoned")
                .output_snapshot();
            for (pane_id, output) in outputs {
                let _ = state_guard.set_pane_output(pane_id, output);
            }
            match state_guard.list_panes(session.as_deref(), window.as_deref()) {
                Ok(panes) => {
                    let target_pane = pane.as_deref();
                    let chosen = panes.into_iter().find(|p| match target_pane {
                        Some(sel) => {
                            sel.parse::<u64>()
                                .map(|id| id == p.pane_id)
                                .unwrap_or(false)
                                || sel == p.pane_title
                        }
                        None => p.active,
                    });
                    if let Some(rec) = chosen {
                        let raw = rec.last_output.unwrap_or_default();
                        let mut all: Vec<String> = raw.lines().map(|l| l.to_string()).collect();
                        if let Some(n) = lines {
                            let len = all.len();
                            all = all.split_off(len.saturating_sub(n));
                        }
                        Response::PaneCaptured {
                            pane_id: rec.pane_id,
                            lines: all,
                        }
                    } else {
                        Response::Error {
                            message: "pane not found".to_string(),
                        }
                    }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        Request::ToggleFloating { session, window, pane } => {
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.toggle_floating(session.as_deref(), window.as_deref(), pane.as_deref()) {
                Ok((_s, _w, p)) => Response::PaneFloated { pane_id: p.id, floating: p.floating },
                Err(e) => Response::Error { message: e.to_string() },
            }
        }
        Request::CompatCommand { name, target, args } => {
            execute_compat_command(state, runtime, &name, target, args)
        }
        Request::ExecuteRaw { command } => match parse_raw_command(&command) {
            Ok(parsed) => handle_request(parsed, state, runtime),
            Err(message) => Response::Error { message },
        },
    }
}

#[derive(Debug, Default, Clone)]
struct ParsedTarget {
    session: Option<String>,
    window: Option<String>,
    pane: Option<String>,
}

fn parse_tmux_target(value: &str) -> ParsedTarget {
    let mut target = ParsedTarget::default();

    if let Some((session, rest)) = value.split_once(':') {
        if !session.is_empty() {
            target.session = Some(session.to_string());
        }
        parse_window_pane(rest, &mut target);
    } else {
        parse_window_pane(value, &mut target);
    }

    target
}

fn core_split_to_proto(node: dmux_core::SplitNode) -> dmux_proto::SplitNodeInfo {
    match node {
        dmux_core::SplitNode::Leaf { pane_id } => dmux_proto::SplitNodeInfo::Leaf { pane_id },
        dmux_core::SplitNode::Split {
            orientation,
            split_ratio,
            first,
            second,
        } => dmux_proto::SplitNodeInfo::Split {
            orientation: match orientation {
                dmux_core::WindowLayout::Horizontal => dmux_proto::SplitOrientation::Horizontal,
                dmux_core::WindowLayout::Vertical => dmux_proto::SplitOrientation::Vertical,
            },
            split_ratio,
            first: Box::new(core_split_to_proto(*first)),
            second: Box::new(core_split_to_proto(*second)),
        },
    }
}

fn compat_command_text(name: &str, target: Option<&str>, args: &[String]) -> String {
    let mut msg = name.replace('_', "-");
    if let Some(t) = target {
        msg.push_str(&format!(" -t {t}"));
    }
    if !args.is_empty() {
        msg.push(' ');
        msg.push_str(&args.join(" "));
    }
    msg
}

fn compat_flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let mut i = 0usize;
    while i + 1 < args.len() {
        if args[i] == flag {
            return Some(args[i + 1].as_str());
        }
        i += 1;
    }
    None
}

fn compat_has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn compat_flag_u16(args: &[String], flag: &str) -> Option<u16> {
    compat_flag_value(args, flag).and_then(|v| v.parse::<u16>().ok())
}

fn execute_compat_command(
    state: Arc<Mutex<ServerState>>,
    runtime: Arc<Mutex<PtyRuntime>>,
    name: &str,
    target: Option<String>,
    args: Vec<String>,
) -> Response {
    let rendered = compat_command_text(name, target.as_deref(), &args);
    let mut compat = compat_state().lock().expect("compat mutex poisoned");
    match name {
        "bind_key" => {
            if args.len() < 2 {
                return Response::Error {
                    message: "bind-key requires key and command".to_string(),
                };
            }
            compat
                .key_bindings
                .insert(args[0].clone(), args[1..].join(" "));
            Response::CommandAccepted {
                command: format!("bind-key {}", args[0]),
            }
        }
        "unbind_key" => {
            if args.is_empty() {
                compat.key_bindings.clear();
                return Response::CommandAccepted {
                    command: "unbind-key all".to_string(),
                };
            }
            compat.key_bindings.remove(&args[0]);
            Response::CommandAccepted {
                command: format!("unbind-key {}", args[0]),
            }
        }
        "list_keys" => Response::Commands(
            compat
                .key_bindings
                .iter()
                .map(|(k, v)| format!("{k} -> {v}"))
                .collect(),
        ),
        "set_buffer" | "load_buffer" => {
            if args.len() < 2 {
                return Response::Error {
                    message: format!("{name} requires name and value"),
                };
            }
            compat.buffers.insert(args[0].clone(), args[1..].join(" "));
            Response::CommandAccepted {
                command: format!("{name} {}", args[0]),
            }
        }
        "delete_buffer" => {
            if args.is_empty() {
                compat.buffers.clear();
                return Response::CommandAccepted {
                    command: "delete-buffer all".to_string(),
                };
            }
            compat.buffers.remove(&args[0]);
            Response::CommandAccepted {
                command: format!("delete-buffer {}", args[0]),
            }
        }
        "list_buffers" => Response::Commands(
            compat
                .buffers
                .iter()
                .map(|(k, v)| format!("{k}:{}", v.len()))
                .collect(),
        ),
        "show_buffer" | "paste_buffer" | "save_buffer" => {
            if args.is_empty() {
                return Response::Error {
                    message: format!("{name} requires buffer name"),
                };
            }
            let line = compat
                .buffers
                .get(&args[0])
                .cloned()
                .unwrap_or_else(String::new);
            Response::Commands(vec![line])
        }
        "set_hook" => {
            if args.len() < 2 {
                return Response::Error {
                    message: "set-hook requires name and command".to_string(),
                };
            }
            compat.hooks.insert(args[0].clone(), args[1..].join(" "));
            Response::CommandAccepted {
                command: format!("set-hook {}", args[0]),
            }
        }
        "show_hooks" => Response::Commands(
            compat
                .hooks
                .iter()
                .map(|(k, v)| format!("{k} -> {v}"))
                .collect(),
        ),
        "set_window_option" => {
            if args.len() < 2 {
                return Response::Error {
                    message: "set-window-option requires name and value".to_string(),
                };
            }
            compat
                .window_options
                .insert(args[0].clone(), args[1..].join(" "));
            Response::CommandAccepted {
                command: format!("set-window-option {}", args[0]),
            }
        }
        "show_window_options" => {
            if let Some(name) = args.first() {
                let v = compat.window_options.get(name).cloned().unwrap_or_default();
                Response::Commands(vec![format!("{name}={v}")])
            } else {
                Response::Commands(
                    compat
                        .window_options
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect(),
                )
            }
        }
        "show_prompt_history" => Response::Commands(compat.prompt_history.clone()),
        "clear_prompt_history" => {
            compat.prompt_history.clear();
            Response::CommandAccepted {
                command: "clear-prompt-history".to_string(),
            }
        }
        "resize_window" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let fallback: Vec<u16> = args.iter().filter_map(|a| a.parse::<u16>().ok()).collect();
            let cols = compat_flag_u16(&args, "-x")
                .or_else(|| fallback.first().copied())
                .unwrap_or(120)
                .max(1);
            let rows = compat_flag_u16(&args, "-y")
                .or_else(|| fallback.get(1).copied())
                .unwrap_or(40)
                .max(1);

            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.resize_window(
                dst.session.as_deref(),
                dst.window.as_deref(),
                rows,
                cols,
            ) {
                Ok((_session, _window, panes)) => {
                    let pane_ids: Vec<u64> = panes.into_iter().map(|p| p.id).collect();
                    drop(state_guard);
                    let runtime_guard = runtime.lock().expect("pty runtime mutex poisoned");
                    for pane_id in pane_ids {
                        let _ = runtime_guard.resize_pty(pane_id, rows, cols);
                    }
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "swap_pane" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let src = compat_flag_value(&args, "-s")
                .map(parse_tmux_target)
                .unwrap_or_default();
            let session = dst.session.clone().or(src.session.clone());
            let window = dst.window.clone().or(src.window.clone());
            if dst.window.is_some() && src.window.is_some() && dst.window != src.window {
                return Response::Error {
                    message: "swap-pane across windows is not implemented".to_string(),
                };
            }

            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.swap_panes(
                session.as_deref(),
                window.as_deref(),
                src.pane.as_deref(),
                dst.pane.as_deref(),
            ) {
                Ok((_session, _window, _pane)) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "rotate_window" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let upward = compat_has_flag(&args, "-U");
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.rotate_window(dst.session.as_deref(), dst.window.as_deref(), upward) {
                Ok((_session, _window, _pane)) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "break_pane" => {
            let src = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let new_name = compat_flag_value(&args, "-n").map(|s| s.to_string());
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.break_pane(
                src.session.as_deref(),
                src.window.as_deref(),
                src.pane.as_deref(),
                new_name,
            ) {
                Ok((_session, _src_window, _dst_window, _pane)) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "join_pane" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let src = compat_flag_value(&args, "-s")
                .map(parse_tmux_target)
                .unwrap_or_default();
            let session = dst.session.clone().or(src.session.clone());
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.join_pane(
                session.as_deref(),
                src.window.as_deref(),
                src.pane.as_deref(),
                dst.window.as_deref(),
                dst.pane.as_deref(),
            ) {
                Ok((_session, _src_window, _dst_window, _pane)) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "respawn_pane" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.respawn_pane(
                dst.session.as_deref(),
                dst.window.as_deref(),
                dst.pane.as_deref(),
            ) {
                Ok((_session, _window, pane)) => {
                    drop(state_guard);
                    let mut runtime_guard = runtime.lock().expect("pty runtime mutex poisoned");
                    if let Err(error) = runtime_guard.respawn_pane(pane.id) {
                        return Response::Error {
                            message: error.to_string(),
                        };
                    }
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "select_layout" | "selectl" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let layout_name = args.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_else(|| "tiled".to_string());
            let Some(layout) = dmux_core::PresetLayout::parse(&layout_name) else {
                return Response::Error { message: format!("unknown layout: {layout_name}") };
            };
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.apply_layout(dst.session.as_deref(), dst.window.as_deref(), layout) {
                Ok(_) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error { message: error.to_string() },
            }
        }
        "next_layout" | "nextl" | "previous_layout" | "prevl" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let forward = !name.starts_with("previous") && name != "prevl";
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.cycle_layout(dst.session.as_deref(), dst.window.as_deref(), forward) {
                Ok((_s, _w, applied)) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: format!("{rendered} -> {}", applied.as_str()) }
                }
                Err(error) => Response::Error { message: error.to_string() },
            }
        }
        "source_file" | "source" => {
            let Some(path) = args.first() else {
                return Response::Error { message: "source-file requires a path".to_string() };
            };
            let path_buf = std::path::PathBuf::from(path);
            drop(compat);
            let contents = match std::fs::read_to_string(&path_buf) {
                Ok(c) => c,
                Err(e) => return Response::Error { message: format!("read {}: {e}", path_buf.display()) },
            };
            let mut executed = 0usize;
            let mut errors = Vec::new();
            for (lineno, line) in contents.lines().enumerate() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let resp = handle_request(
                    Request::ExecuteRaw { command: trimmed.to_string() },
                    Arc::clone(&state),
                    Arc::clone(&runtime),
                );
                if let Response::Error { message } = resp {
                    errors.push(format!("line {}: {message}", lineno + 1));
                } else {
                    executed += 1;
                }
            }
            let mut compat = compat_state().lock().expect("compat mutex poisoned");
            compat.prompt_history.push(rendered.clone());
            if errors.is_empty() {
                Response::CommandAccepted { command: format!("{rendered}: executed {executed}") }
            } else {
                Response::Error { message: format!("source-file: {} errors: {}", errors.len(), errors.join("; ")) }
            }
        }
        "run_shell" | "run" => {
            let Some(shell_cmd) = args.first() else {
                return Response::Error { message: "run-shell requires a command".to_string() };
            };
            let cmd = shell_cmd.clone();
            drop(compat);
            let output = match std::process::Command::new("sh").arg("-c").arg(&cmd).output() {
                Ok(o) => o,
                Err(e) => return Response::Error { message: format!("run-shell spawn: {e}") },
            };
            let mut compat = compat_state().lock().expect("compat mutex poisoned");
            compat.prompt_history.push(rendered.clone());
            let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout).lines().map(|s| s.to_string()).collect();
            if !output.stderr.is_empty() {
                for l in String::from_utf8_lossy(&output.stderr).lines() {
                    lines.push(format!("stderr: {l}"));
                }
            }
            Response::Commands(lines)
        }
        "if_shell" | "if" => {
            // if-shell <cond> <true-cmd> [false-cmd]
            if args.len() < 2 {
                return Response::Error { message: "if-shell requires <cond> <true-cmd>".to_string() };
            }
            let cond = args[0].clone();
            let true_cmd = args[1].clone();
            let false_cmd = args.get(2).cloned();
            drop(compat);
            let status = std::process::Command::new("sh").arg("-c").arg(&cond).status();
            let branch = match status {
                Ok(s) if s.success() => Some(true_cmd),
                _ => false_cmd,
            };
            let mut compat = compat_state().lock().expect("compat mutex poisoned");
            compat.prompt_history.push(rendered.clone());
            drop(compat);
            if let Some(cmd) = branch {
                handle_request(
                    Request::ExecuteRaw { command: cmd },
                    Arc::clone(&state),
                    Arc::clone(&runtime),
                )
            } else {
                Response::CommandAccepted { command: format!("{rendered}: no branch") }
            }
        }
        "pipe_pane" | "pipep" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let toggle = compat_has_flag(&args, "-o");
            let shell_cmd: Option<String> = args.iter().rev()
                .find(|a| !a.starts_with('-'))
                .cloned();
            let state_guard = state.lock().expect("state mutex poisoned");
            let pane_id = match state_guard.list_panes(dst.session.as_deref(), dst.window.as_deref()) {
                Ok(panes) => {
                    let target_pane = if let Some(p) = dst.pane.as_deref() {
                        panes.iter().find(|r| r.pane_id.to_string() == p || r.pane_title == p).map(|r| r.pane_id)
                    } else {
                        panes.iter().find(|r| r.active).map(|r| r.pane_id)
                    };
                    match target_pane {
                        Some(id) => id,
                        None => return Response::Error { message: "pipe-pane: no pane".to_string() },
                    }
                }
                Err(e) => return Response::Error { message: e.to_string() },
            };
            drop(state_guard);
            let runtime_guard = runtime.lock().expect("pty runtime mutex poisoned");
            let cmd_to_use = if toggle { shell_cmd } else { shell_cmd };
            match runtime_guard.toggle_pipe(pane_id, cmd_to_use) {
                Ok(msg) => {
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: msg.unwrap_or(rendered) }
                }
                Err(e) => Response::Error { message: e.to_string() },
            }
        }
        "display_menu" | "display_panes" | "display_popup" | "clock_mode" | "copy_mode"
        | "customize_mode" | "confirm_before" | "command_prompt" | "refresh_client"
        | "send_prefix" | "server_access" | "swap_window"
        | "choose_buffer" | "choose_client" | "choose_tree" | "link_window"
        | "move_pane" | "move_window"
        | "resize_pane" | "respawn_window" | "unlink_window" => {
            compat.prompt_history.push(rendered.clone());
            Response::CommandAccepted { command: rendered }
        }
        "capture_pane" => {
            let state = state.lock().expect("state mutex poisoned");
            let lines = if let Some(session) = state.sessions().first() {
                if let Some(window) = session
                    .windows
                    .iter()
                    .find(|w| w.id == session.active_window_id)
                {
                    window
                        .panes
                        .iter()
                        .filter_map(|p| p.last_input.clone())
                        .collect::<Vec<_>>()
                } else {
                    vec![]
                }
            } else {
                vec![]
            };
            Response::Commands(lines)
        }
        "clear_history" => {
            let dst = target.as_deref().map(parse_tmux_target).unwrap_or_default();
            let mut state_guard = state.lock().expect("state mutex poisoned");
            match state_guard.clear_pane_history(
                dst.session.as_deref(),
                dst.window.as_deref(),
                dst.pane.as_deref(),
            ) {
                Ok((_session, _window, pane)) => {
                    drop(state_guard);
                    runtime
                        .lock()
                        .expect("pty runtime mutex poisoned")
                        .clear_history(pane.id);
                    compat.prompt_history.push(rendered.clone());
                    Response::CommandAccepted { command: rendered }
                }
                Err(error) => Response::Error {
                    message: error.to_string(),
                },
            }
        }
        "find_window" => {
            let query = args.first().cloned().unwrap_or_default();
            let state = state.lock().expect("state mutex poisoned");
            let mut lines = vec![];
            for s in state.sessions() {
                for w in &s.windows {
                    if query.is_empty() || w.name.contains(&query) {
                        lines.push(format!("{}:{}", s.name, w.name));
                    }
                }
            }
            Response::Commands(lines)
        }
        "wait_for" => {
            if args.is_empty() {
                return Response::Error {
                    message: "wait-for requires a token".to_string(),
                };
            }
            let token = args.last().cloned().unwrap_or_default();
            if args.iter().any(|a| a == "-S") {
                compat.wait_tokens.insert(token.clone());
                Response::CommandAccepted {
                    command: format!("wait-for signalled {token}"),
                }
            } else if compat.wait_tokens.remove(&token) {
                Response::CommandAccepted {
                    command: format!("wait-for consumed {token}"),
                }
            } else {
                Response::CommandAccepted {
                    command: format!("wait-for would-wait {token}"),
                }
            }
        }
        _ => Response::CommandAccepted {
            command: format!("compat handled {name}"),
        },
    }
}

fn parse_window_pane(value: &str, target: &mut ParsedTarget) {
    if value.is_empty() {
        return;
    }

    if let Some((window, pane)) = value.split_once('.') {
        if !window.is_empty() {
            target.window = Some(window.to_string());
        }
        if !pane.is_empty() {
            target.pane = Some(pane.to_string());
        }
    } else {
        target.window = Some(value.to_string());
    }
}

fn parse_raw_command(command: &str) -> std::result::Result<Request, String> {
    let tokens: Vec<&str> = command.split_whitespace().collect();
    if tokens.is_empty() {
        return Err("empty command".to_string());
    }

    match tokens[0] {
        "new-session" | "new" => {
            let mut name = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-s" if i + 1 < tokens.len() => {
                        name = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    t if !t.starts_with('-') && name.is_none() => {
                        name = Some(t.to_string());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            Ok(Request::CreateSession {
                name: name.unwrap_or_else(|| "default".to_string()),
            })
        }
        "has-session" | "has" => {
            let mut target = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        target = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    t if !t.starts_with('-') && target.is_none() => {
                        target = Some(t.to_string());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            match target {
                Some(name) => Ok(Request::HasSession { name }),
                None => Err("has-session requires a target session".to_string()),
            }
        }
        "attach-session" | "attach" => {
            let session = parse_session_target_flag(&tokens[1..]);
            Ok(Request::AttachSession { session })
        }
        "switch-client" | "switchc" => {
            let session = parse_session_target_flag(&tokens[1..]);
            Ok(Request::SwitchClient { session })
        }
        "detach-client" | "detach" => {
            let session = parse_session_target_flag(&tokens[1..]);
            Ok(Request::DetachClient { session })
        }
        "list-clients" | "lsc" => Ok(Request::ListClients),
        "lock-client" | "lockc" => Ok(Request::LockClient),
        "lock-server" | "locks" => Ok(Request::LockServer),
        "lock-session" => Ok(Request::LockSession {
            session: parse_session_target_flag(&tokens[1..]),
        }),
        "suspend-client" | "suspendc" => Ok(Request::SuspendClient),
        "display-message" | "display" => Ok(Request::DisplayMessage {
            message: parse_message_args(&tokens[1..])?,
        }),
        "show-messages" => Ok(Request::ShowMessages),
        "set-option" | "set" => parse_set_option(&tokens[1..]),
        "show-options" | "show" => Ok(Request::ShowOptions {
            name: parse_name_arg(&tokens[1..]),
        }),
        "set-environment" | "setenv" => parse_set_environment(&tokens[1..]),
        "show-environment" | "showenv" => Ok(Request::ShowEnvironment {
            name: parse_name_arg(&tokens[1..]),
        }),
        "list-sessions" | "ls" => Ok(Request::ListSessions),
        "list-commands" | "lscm" => Ok(Request::ListCommands),
        "kill-server" => Ok(Request::KillServer),
        "start-server" => Ok(Request::StartServer),
        "kill-session" | "kill" => {
            let mut target = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        target = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    t if !t.starts_with('-') && target.is_none() => {
                        target = Some(t.to_string());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            match target {
                Some(name) => Ok(Request::KillSession { name }),
                None => Err("kill-session requires a target session".to_string()),
            }
        }
        "rename-session" => {
            let mut target = None;
            let mut new_name = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        target = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    "-n" if i + 1 < tokens.len() => {
                        new_name = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    t if !t.starts_with('-') && target.is_none() => {
                        target = Some(t.to_string());
                        i += 1;
                    }
                    t if !t.starts_with('-') && new_name.is_none() => {
                        new_name = Some(t.to_string());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            match (target, new_name) {
                (Some(name), Some(new_name)) => Ok(Request::RenameSession { name, new_name }),
                _ => Err("rename-session requires target and new name".to_string()),
            }
        }
        "new-window" => {
            let mut target = None;
            let mut name = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        target = Some(tokens[i + 1]);
                        i += 2;
                    }
                    "-n" if i + 1 < tokens.len() => {
                        name = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            let parsed = target.map(parse_tmux_target);
            let session = parsed.and_then(|t| t.session);
            Ok(Request::CreateWindow { session, name })
        }
        "rename-window" => {
            let mut parsed = ParsedTarget::default();
            let mut new_name = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        parsed = parse_tmux_target(tokens[i + 1]);
                        i += 2;
                    }
                    "-n" if i + 1 < tokens.len() => {
                        new_name = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    t if !t.starts_with('-') && new_name.is_none() => {
                        new_name = Some(t.to_string());
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            match new_name {
                Some(new_name) => Ok(Request::RenameWindow {
                    session: parsed.session,
                    window: parsed.window,
                    new_name,
                }),
                None => Err("rename-window requires a new name".to_string()),
            }
        }
        "kill-window" | "killw" => {
            let parsed = parse_target_flag(&tokens[1..]);
            Ok(Request::KillWindow {
                session: parsed.session,
                window: parsed.window,
            })
        }
        "next-window" | "nextw" => Ok(Request::NextWindow {
            session: parse_session_target_flag(&tokens[1..]),
        }),
        "previous-window" | "prevw" => Ok(Request::PreviousWindow {
            session: parse_session_target_flag(&tokens[1..]),
        }),
        "last-window" | "last" => Ok(Request::LastWindow {
            session: parse_session_target_flag(&tokens[1..]),
        }),
        "select-window" | "selectw" => {
            let parsed = parse_target_flag(&tokens[1..]);
            match parsed.window {
                Some(window) => Ok(Request::SelectWindow {
                    session: parsed.session,
                    window,
                }),
                None => Err(
                    "select-window requires a window target (eg. -t session:window)".to_string(),
                ),
            }
        }
        "split-window" | "splitw" => {
            let mut parsed = ParsedTarget::default();
            let mut title = None;
            let mut orientation = dmux_proto::SplitOrientation::Horizontal;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        parsed = parse_tmux_target(tokens[i + 1]);
                        i += 2;
                    }
                    "-h" => {
                        orientation = dmux_proto::SplitOrientation::Vertical;
                        i += 1;
                    }
                    "-v" => {
                        orientation = dmux_proto::SplitOrientation::Horizontal;
                        i += 1;
                    }
                    "-n" if i + 1 < tokens.len() => {
                        title = Some(tokens[i + 1].to_string());
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            Ok(Request::SplitWindow {
                session: parsed.session,
                window: parsed.window,
                title,
                orientation,
            })
        }
        "kill-pane" | "killp" => {
            let parsed = parse_target_flag(&tokens[1..]);
            Ok(Request::KillPane {
                session: parsed.session,
                window: parsed.window,
                pane: parsed.pane,
            })
        }
        "select-pane" | "selectp" => {
            let parsed = parse_target_flag(&tokens[1..]);
            match parsed.pane {
                Some(pane) => Ok(Request::SelectPane {
                    session: parsed.session,
                    window: parsed.window,
                    pane,
                }),
                None => {
                    Err("select-pane requires pane target (eg. -t session:window.pane)".to_string())
                }
            }
        }
        "last-pane" | "lastp" => {
            let parsed = parse_target_flag(&tokens[1..]);
            Ok(Request::LastPane {
                session: parsed.session,
                window: parsed.window,
            })
        }
        "send-keys" => {
            let mut parsed = ParsedTarget::default();
            let mut literal = false;
            let mut keys = Vec::new();
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        parsed = parse_tmux_target(tokens[i + 1]);
                        i += 2;
                    }
                    "-l" => {
                        literal = true;
                        i += 1;
                    }
                    other => {
                        keys.push(other.to_string());
                        i += 1;
                    }
                }
            }
            if keys.is_empty() {
                return Err("send-keys requires at least one key token".to_string());
            }
            Ok(Request::SendKeys {
                session: parsed.session,
                window: parsed.window,
                pane: parsed.pane,
                keys,
                literal,
            })
        }
        "list-windows" | "lsw" => {
            let parsed = parse_target_flag(&tokens[1..]);
            Ok(Request::ListWindows {
                session: parsed
                    .session
                    .or_else(|| parsed.window.filter(|_| parsed.pane.is_none())),
            })
        }
        "list-panes" | "lsp" => {
            let parsed = parse_target_flag(&tokens[1..]);
            Ok(Request::ListPanes {
                session: parsed.session,
                window: parsed.window,
            })
        }
        "resize-pane" | "resizep" => {
            let mut parsed = ParsedTarget::default();
            let mut direction = dmux_proto::ResizeDir::Down;
            let mut amount: u16 = 1;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        parsed = parse_tmux_target(tokens[i + 1]);
                        i += 2;
                    }
                    "-U" => {
                        direction = dmux_proto::ResizeDir::Up;
                        i += 1;
                    }
                    "-D" => {
                        direction = dmux_proto::ResizeDir::Down;
                        i += 1;
                    }
                    "-L" => {
                        direction = dmux_proto::ResizeDir::Left;
                        i += 1;
                    }
                    "-R" => {
                        direction = dmux_proto::ResizeDir::Right;
                        i += 1;
                    }
                    other => {
                        if let Ok(n) = other.parse::<u16>() {
                            amount = n;
                        }
                        i += 1;
                    }
                }
            }
            Ok(Request::ResizePane {
                session: parsed.session,
                window: parsed.window,
                pane: parsed.pane,
                direction,
                amount,
            })
        }
        "capture-pane" | "capturep" => {
            let mut parsed = ParsedTarget::default();
            let mut lines: Option<usize> = None;
            let mut i = 1usize;
            while i < tokens.len() {
                match tokens[i] {
                    "-t" if i + 1 < tokens.len() => {
                        parsed = parse_tmux_target(tokens[i + 1]);
                        i += 2;
                    }
                    "-S" if i + 1 < tokens.len() => {
                        if let Ok(n) = tokens[i + 1].parse::<i64>() {
                            lines = Some(n.unsigned_abs() as usize);
                        }
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            Ok(Request::CapturePane {
                session: parsed.session,
                window: parsed.window,
                pane: parsed.pane,
                lines,
            })
        }
        unsupported => {
            if let Some(request) = parse_compat_fallback(unsupported, &tokens[1..]) {
                Ok(request)
            } else {
                Err(format!(
                    "unsupported raw command '{unsupported}' in dmux compatibility shim"
                ))
            }
        }
    }
}

fn parse_target_flag(tokens: &[&str]) -> ParsedTarget {
    let mut parsed = ParsedTarget::default();
    let mut i = 0usize;
    while i < tokens.len() {
        match tokens[i] {
            "-t" if i + 1 < tokens.len() => {
                parsed = parse_tmux_target(tokens[i + 1]);
                i += 2;
            }
            _ => i += 1,
        }
    }
    parsed
}

fn parse_session_target_flag(tokens: &[&str]) -> Option<String> {
    let mut i = 0usize;
    while i < tokens.len() {
        match tokens[i] {
            "-t" if i + 1 < tokens.len() => {
                return session_from_raw_target(tokens[i + 1]);
            }
            t if !t.starts_with('-') => {
                return session_from_raw_target(t);
            }
            _ => i += 1,
        }
    }
    None
}

fn session_from_raw_target(target: &str) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    if let Some((session, _rest)) = target.split_once(':') {
        if session.is_empty() {
            None
        } else {
            Some(session.to_string())
        }
    } else {
        Some(target.to_string())
    }
}

fn parse_message_args(tokens: &[&str]) -> std::result::Result<String, String> {
    let parts: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !t.starts_with('-'))
        .collect();
    if parts.is_empty() {
        return Err("display-message requires a message payload".to_string());
    }
    Ok(parts.join(" "))
}

fn parse_name_arg(tokens: &[&str]) -> Option<String> {
    tokens
        .iter()
        .copied()
        .find(|t| !t.starts_with('-'))
        .map(|t| t.to_string())
}

fn parse_set_option(tokens: &[&str]) -> std::result::Result<Request, String> {
    let parts: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !t.starts_with('-'))
        .collect();
    if parts.len() < 2 {
        return Err("set-option requires name and value".to_string());
    }
    Ok(Request::SetOption {
        name: parts[0].to_string(),
        value: parts[1..].join(" "),
    })
}

fn parse_set_environment(tokens: &[&str]) -> std::result::Result<Request, String> {
    let parts: Vec<&str> = tokens
        .iter()
        .copied()
        .filter(|t| !t.starts_with('-'))
        .collect();
    if parts.len() < 2 {
        return Err("set-environment requires name and value".to_string());
    }
    Ok(Request::SetEnvironment {
        name: parts[0].to_string(),
        value: parts[1..].join(" "),
    })
}

fn parse_compat_fallback(cmd: &str, rest: &[&str]) -> Option<Request> {
    let name = cmd.replace('-', "_");
    let supported: HashSet<&'static str> = HashSet::from([
        "bind_key",
        "break_pane",
        "capture_pane",
        "choose_buffer",
        "choose_client",
        "choose_tree",
        "clear_history",
        "clear_prompt_history",
        "clock_mode",
        "command_prompt",
        "confirm_before",
        "copy_mode",
        "customize_mode",
        "delete_buffer",
        "display_menu",
        "display_panes",
        "display_popup",
        "find_window",
        "if_shell",
        "join_pane",
        "link_window",
        "list_buffers",
        "list_keys",
        "load_buffer",
        "move_pane",
        "move_window",
        "next_layout",
        "paste_buffer",
        "pipe_pane",
        "previous_layout",
        "refresh_client",
        "resize_pane",
        "resize_window",
        "respawn_pane",
        "respawn_window",
        "rotate_window",
        "run_shell",
        "save_buffer",
        "select_layout",
        "send_prefix",
        "server_access",
        "set_buffer",
        "set_hook",
        "set_window_option",
        "show_buffer",
        "show_hooks",
        "show_prompt_history",
        "show_window_options",
        "source_file",
        "swap_pane",
        "swap_window",
        "unbind_key",
        "unlink_window",
        "wait_for",
    ]);
    if !supported.contains(name.as_str()) {
        return None;
    }
    let (target, args) = extract_target_and_args(rest);
    Some(Request::CompatCommand { name, target, args })
}

fn extract_target_and_args(tokens: &[&str]) -> (Option<String>, Vec<String>) {
    let mut target = None;
    let mut args = vec![];
    let mut i = 0usize;
    while i < tokens.len() {
        if tokens[i] == "-t" && i + 1 < tokens.len() {
            target = Some(tokens[i + 1].to_string());
            i += 2;
            continue;
        }
        args.push(tokens[i].to_string());
        i += 1;
    }
    (target, args)
}
