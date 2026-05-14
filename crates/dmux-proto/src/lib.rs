use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    Ping,
    CreateSession {
        name: String,
    },
    HasSession {
        name: String,
    },
    AttachSession {
        session: Option<String>,
    },
    SwitchClient {
        session: Option<String>,
    },
    DetachClient {
        session: Option<String>,
    },
    KillSession {
        name: String,
    },
    RenameSession {
        name: String,
        new_name: String,
    },
    CreateWindow {
        session: Option<String>,
        name: Option<String>,
    },
    RenameWindow {
        session: Option<String>,
        window: Option<String>,
        new_name: String,
    },
    KillWindow {
        session: Option<String>,
        window: Option<String>,
    },
    NextWindow {
        session: Option<String>,
    },
    PreviousWindow {
        session: Option<String>,
    },
    LastWindow {
        session: Option<String>,
    },
    SelectWindow {
        session: Option<String>,
        window: String,
    },
    SplitWindow {
        session: Option<String>,
        window: Option<String>,
        title: Option<String>,
        orientation: SplitOrientation,
    },
    KillPane {
        session: Option<String>,
        window: Option<String>,
        pane: Option<String>,
    },
    SelectPane {
        session: Option<String>,
        window: Option<String>,
        pane: String,
    },
    LastPane {
        session: Option<String>,
        window: Option<String>,
    },
    LockClient,
    LockServer,
    LockSession {
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
        name: Option<String>,
    },
    SetEnvironment {
        name: String,
        value: String,
    },
    ShowEnvironment {
        name: Option<String>,
    },
    CompatCommand {
        name: String,
        target: Option<String>,
        args: Vec<String>,
    },
    KillServer,
    StartServer,
    SendKeys {
        session: Option<String>,
        window: Option<String>,
        pane: Option<String>,
        keys: Vec<String>,
        literal: bool,
    },
    ListSessions,
    ListCommands,
    ListClients,
    ListWindows {
        session: Option<String>,
    },
    ListPanes {
        session: Option<String>,
        window: Option<String>,
    },
    ResizePane {
        session: Option<String>,
        window: Option<String>,
        pane: Option<String>,
        direction: ResizeDir,
        amount: u16,
    },
    CapturePane {
        session: Option<String>,
        window: Option<String>,
        pane: Option<String>,
        lines: Option<usize>,
    },
    ToggleFloating {
        session: Option<String>,
        window: Option<String>,
        pane: Option<String>,
    },
    ExecuteRaw {
        command: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResizeDir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SplitOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplitNodeInfo {
    Leaf {
        pane_id: u64,
    },
    Split {
        orientation: SplitOrientation,
        #[serde(default = "default_split_ratio")]
        split_ratio: u16,
        first: Box<SplitNodeInfo>,
        second: Box<SplitNodeInfo>,
    },
}

const fn default_split_ratio() -> u16 {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Response {
    Pong,
    SessionCheck {
        name: String,
        exists: bool,
    },
    SessionAttached {
        name: String,
    },
    ClientSwitched {
        from: Option<String>,
        to: String,
    },
    ClientDetached {
        name: String,
    },
    SessionCreated {
        id: u64,
        name: String,
    },
    SessionKilled {
        id: u64,
        name: String,
    },
    SessionRenamed {
        id: u64,
        old_name: String,
        new_name: String,
    },
    WindowCreated {
        session: String,
        id: u64,
        name: String,
    },
    WindowRenamed {
        session: String,
        id: u64,
        old_name: String,
        new_name: String,
    },
    WindowKilled {
        session: String,
        id: u64,
        name: String,
    },
    WindowSelected {
        session: String,
        id: u64,
        name: String,
    },
    PaneResized {
        session: String,
        window_id: u64,
        pane_id: u64,
        rows: u16,
        cols: u16,
    },
    PaneCaptured {
        pane_id: u64,
        lines: Vec<String>,
    },
    PaneFloated {
        pane_id: u64,
        floating: bool,
    },
    PaneCreated {
        session: String,
        window_id: u64,
        pane_id: u64,
        title: String,
    },
    PaneKilled {
        session: String,
        window_id: u64,
        pane_id: u64,
        title: String,
    },
    PaneSelected {
        session: String,
        window_id: u64,
        pane_id: u64,
        title: String,
    },
    KeysSent {
        session: String,
        window_id: u64,
        pane_id: u64,
        payload: String,
    },
    Sessions(Vec<SessionInfo>),
    Commands(Vec<String>),
    Clients(Vec<ClientInfo>),
    ClientLocked {
        name: String,
    },
    ClientSuspended {
        name: String,
    },
    ServerLocked,
    SessionLocked {
        name: String,
    },
    MessageDisplayed {
        message: String,
    },
    Messages(Vec<String>),
    OptionSet {
        name: String,
        value: String,
    },
    Options(Vec<NameValue>),
    EnvironmentSet {
        name: String,
        value: String,
    },
    Environment(Vec<NameValue>),
    Windows(Vec<WindowInfo>),
    Panes(Vec<PaneInfo>),
    CommandAccepted {
        command: String,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub id: u64,
    pub name: String,
    pub windows: usize,
    pub panes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    pub session_id: u64,
    pub session_name: String,
    pub window_id: u64,
    pub window_name: String,
    pub pane_count: usize,
    pub active: bool,
    #[serde(default)]
    pub layout: SplitOrientation,
    #[serde(default)]
    pub layout_tree: Option<SplitNodeInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaneInfo {
    pub session_id: u64,
    pub session_name: String,
    pub window_id: u64,
    pub window_name: String,
    pub pane_id: u64,
    pub pane_title: String,
    pub active: bool,
    pub last_input: Option<String>,
    pub last_output: Option<String>,
    #[serde(default)]
    pub rows: u16,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub floating: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub id: u64,
    pub name: String,
    pub attached_session: Option<String>,
    pub locked: bool,
    pub suspended: bool,
    pub server_locked: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NameValue {
    pub name: String,
    pub value: String,
}
