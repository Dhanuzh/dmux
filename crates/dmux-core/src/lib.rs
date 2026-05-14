use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: u64,
    pub name: String,
    pub active_window_id: u64,
    pub last_window_id: Option<u64>,
    pub windows: Vec<Window>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Window {
    pub id: u64,
    pub name: String,
    pub active_pane_id: u64,
    pub last_pane_id: Option<u64>,
    #[serde(default)]
    pub layout: WindowLayout,
    #[serde(default)]
    pub layout_tree: Option<SplitNode>,
    #[serde(default)]
    pub current_layout: PresetLayout,
    pub panes: Vec<Pane>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WindowLayout {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PresetLayout {
    #[default]
    EvenHorizontal,
    EvenVertical,
    MainHorizontal,
    MainVertical,
    Tiled,
}

impl PresetLayout {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "even-horizontal" | "eh" => Some(Self::EvenHorizontal),
            "even-vertical" | "ev" => Some(Self::EvenVertical),
            "main-horizontal" | "mh" => Some(Self::MainHorizontal),
            "main-vertical" | "mv" => Some(Self::MainVertical),
            "tiled" => Some(Self::Tiled),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EvenHorizontal => "even-horizontal",
            Self::EvenVertical => "even-vertical",
            Self::MainHorizontal => "main-horizontal",
            Self::MainVertical => "main-vertical",
            Self::Tiled => "tiled",
        }
    }
}

fn build_preset_layout(layout: PresetLayout, pane_ids: &[u64]) -> (WindowLayout, SplitNode) {
    match layout {
        PresetLayout::EvenHorizontal => (WindowLayout::Vertical, even_split(pane_ids, WindowLayout::Vertical)),
        PresetLayout::EvenVertical => (WindowLayout::Horizontal, even_split(pane_ids, WindowLayout::Horizontal)),
        PresetLayout::MainHorizontal => (WindowLayout::Horizontal, main_split(pane_ids, WindowLayout::Horizontal)),
        PresetLayout::MainVertical => (WindowLayout::Vertical, main_split(pane_ids, WindowLayout::Vertical)),
        PresetLayout::Tiled => (WindowLayout::Horizontal, tiled_split(pane_ids)),
    }
}

fn even_split(ids: &[u64], orient: WindowLayout) -> SplitNode {
    if ids.len() == 1 {
        return SplitNode::Leaf { pane_id: ids[0] };
    }
    let (head, tail) = ids.split_at(1);
    SplitNode::Split {
        orientation: orient,
        split_ratio: (1000 / ids.len() as u16).max(50),
        first: Box::new(SplitNode::Leaf { pane_id: head[0] }),
        second: Box::new(even_split(tail, orient)),
    }
}

fn main_split(ids: &[u64], orient: WindowLayout) -> SplitNode {
    if ids.len() == 1 {
        return SplitNode::Leaf { pane_id: ids[0] };
    }
    let (head, tail) = ids.split_at(1);
    let other_orient = match orient {
        WindowLayout::Horizontal => WindowLayout::Vertical,
        WindowLayout::Vertical => WindowLayout::Horizontal,
    };
    SplitNode::Split {
        orientation: orient,
        split_ratio: 600,
        first: Box::new(SplitNode::Leaf { pane_id: head[0] }),
        second: Box::new(even_split(tail, other_orient)),
    }
}

fn tiled_split(ids: &[u64]) -> SplitNode {
    if ids.len() == 1 {
        return SplitNode::Leaf { pane_id: ids[0] };
    }
    let cols = (ids.len() as f64).sqrt().ceil() as usize;
    let rows = (ids.len() + cols - 1) / cols;
    let mut row_nodes: Vec<SplitNode> = Vec::new();
    for r in 0..rows {
        let start = r * cols;
        let end = ((r + 1) * cols).min(ids.len());
        let row_ids = &ids[start..end];
        row_nodes.push(even_split(row_ids, WindowLayout::Vertical));
    }
    even_split_nodes(row_nodes, WindowLayout::Horizontal)
}

fn even_split_nodes(mut nodes: Vec<SplitNode>, orient: WindowLayout) -> SplitNode {
    if nodes.len() == 1 {
        return nodes.remove(0);
    }
    let head = nodes.remove(0);
    SplitNode::Split {
        orientation: orient,
        split_ratio: (1000 / (nodes.len() as u16 + 1)).max(50),
        first: Box::new(head),
        second: Box::new(even_split_nodes(nodes, orient)),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SplitNode {
    Leaf {
        pane_id: u64,
    },
    Split {
        orientation: WindowLayout,
        #[serde(default = "default_split_ratio")]
        split_ratio: u16,
        first: Box<SplitNode>,
        second: Box<SplitNode>,
    },
}

const fn default_split_ratio() -> u16 {
    500
}

const MIN_PANE_AXIS_SIZE: usize = 3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResizeDirection {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pane {
    pub id: u64,
    pub title: String,
    pub last_input: Option<String>,
    pub last_output: Option<String>,
    #[serde(default)]
    pub rows: u16,
    #[serde(default)]
    pub cols: u16,
    #[serde(default)]
    pub floating: bool,
}

impl ServerState {
    pub fn toggle_floating(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;
        window.panes[pane_index].floating = !window.panes[pane_index].floating;
        let pane = window.panes[pane_index].clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientState {
    pub id: u64,
    pub name: String,
    pub attached_session: Option<String>,
    pub locked: bool,
    pub suspended: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ServerState {
    next_session_id: u64,
    next_window_id: u64,
    next_pane_id: u64,
    attached_session: Option<String>,
    client: ClientState,
    server_locked: bool,
    locked_sessions: HashSet<String>,
    options: BTreeMap<String, String>,
    environment: BTreeMap<String, String>,
    message_log: Vec<String>,
    sessions: Vec<Session>,
}

impl ServerState {
    pub fn to_json(&self) -> std::result::Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }
    pub fn from_json(s: &str) -> std::result::Result<Self, String> {
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
    pub fn pane_ids(&self) -> Vec<u64> {
        self.sessions.iter().flat_map(|s| s.windows.iter().flat_map(|w| w.panes.iter().map(|p| p.id))).collect()
    }
}

#[derive(Debug, Error)]
pub enum StateError {
    #[error("session '{0}' already exists")]
    SessionExists(String),
    #[error("session '{0}' not found")]
    SessionNotFound(String),
    #[error("window target '{0}' not found")]
    WindowNotFound(String),
    #[error("pane target '{0}' not found")]
    PaneNotFound(String),
    #[error("no sessions exist")]
    NoSessions,
    #[error("session '{0}' has no windows")]
    NoWindows(String),
    #[error("window '{0}' has no panes")]
    NoPanes(String),
    #[error("cannot kill the last window in session '{0}'")]
    LastWindowInSession(String),
    #[error("cannot kill the last pane in window '{0}'")]
    LastPaneInWindow(String),
    #[error("no attached session")]
    NoAttachedSession,
}

impl ServerState {
    pub fn new() -> Self {
        Self {
            next_session_id: 1,
            next_window_id: 1,
            next_pane_id: 1,
            attached_session: None,
            client: ClientState {
                id: 1,
                name: "local-client".to_string(),
                attached_session: None,
                locked: false,
                suspended: false,
            },
            server_locked: false,
            locked_sessions: HashSet::new(),
            options: BTreeMap::new(),
            environment: BTreeMap::new(),
            message_log: Vec::new(),
            sessions: Vec::new(),
        }
    }

    pub fn create_session(&mut self, name: String) -> Result<Session, StateError> {
        if self.sessions.iter().any(|s| s.name == name) {
            return Err(StateError::SessionExists(name));
        }

        let pane = Pane {
            id: self.next_pane_id,
            title: "shell".to_string(),
            last_input: None,
            last_output: None,
            rows: 0,
            cols: 0,
            floating: false,
        };
        self.next_pane_id += 1;

        let window = Window {
            id: self.next_window_id,
            name: "0".to_string(),
            active_pane_id: pane.id,
            last_pane_id: None,
            layout: WindowLayout::Horizontal,
            layout_tree: Some(SplitNode::Leaf { pane_id: pane.id }),
            current_layout: PresetLayout::EvenHorizontal,
            panes: vec![pane],
        };
        self.next_window_id += 1;

        let session = Session {
            id: self.next_session_id,
            name,
            active_window_id: window.id,
            last_window_id: None,
            windows: vec![window],
        };
        self.next_session_id += 1;

        self.sessions.push(session.clone());
        if self.attached_session.is_none() {
            self.attached_session = Some(session.name.clone());
            self.client.attached_session = Some(session.name.clone());
        }
        Ok(session)
    }

    pub fn attach_session(&mut self, session_name: Option<&str>) -> Result<Session, StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = self.sessions[session_index].clone();
        self.attached_session = Some(session.name.clone());
        self.client.attached_session = Some(session.name.clone());
        Ok(session)
    }

    pub fn switch_client(
        &mut self,
        session_name: Option<&str>,
    ) -> Result<(Option<String>, Session), StateError> {
        let previous = self.attached_session.clone();
        let session = self.attach_session(session_name)?;
        Ok((previous, session))
    }

    pub fn detach_client(&mut self, session_name: Option<&str>) -> Result<String, StateError> {
        if let Some(target) = session_name {
            let _ = self.find_session_index(Some(target))?;
            if self.attached_session.as_deref() == Some(target) {
                self.attached_session = None;
                self.client.attached_session = None;
                return Ok(target.to_string());
            }
            return Err(StateError::NoAttachedSession);
        }
        match self.attached_session.take() {
            Some(name) => {
                self.client.attached_session = None;
                Ok(name)
            }
            None => Err(StateError::NoAttachedSession),
        }
    }

    pub fn sessions(&self) -> &[Session] {
        &self.sessions
    }

    pub fn list_clients(&self) -> Vec<ClientRecord> {
        vec![ClientRecord {
            id: self.client.id,
            name: self.client.name.clone(),
            attached_session: self.client.attached_session.clone(),
            locked: self.client.locked,
            suspended: self.client.suspended,
            server_locked: self.server_locked,
        }]
    }

    pub fn lock_client(&mut self) -> ClientRecord {
        self.client.locked = true;
        ClientRecord {
            id: self.client.id,
            name: self.client.name.clone(),
            attached_session: self.client.attached_session.clone(),
            locked: self.client.locked,
            suspended: self.client.suspended,
            server_locked: self.server_locked,
        }
    }

    pub fn suspend_client(&mut self) -> ClientRecord {
        self.client.suspended = true;
        ClientRecord {
            id: self.client.id,
            name: self.client.name.clone(),
            attached_session: self.client.attached_session.clone(),
            locked: self.client.locked,
            suspended: self.client.suspended,
            server_locked: self.server_locked,
        }
    }

    pub fn lock_server(&mut self) -> bool {
        self.server_locked = true;
        self.server_locked
    }

    pub fn lock_session(&mut self, session_name: Option<&str>) -> Result<String, StateError> {
        let session_index = self.find_session_index(session_name)?;
        let name = self.sessions[session_index].name.clone();
        self.locked_sessions.insert(name.clone());
        Ok(name)
    }

    pub fn display_message(&mut self, message: String) -> String {
        self.push_message(message.clone());
        message
    }

    pub fn show_messages(&self) -> Vec<String> {
        self.message_log.clone()
    }

    pub fn set_option(&mut self, name: &str, value: &str) -> (String, String) {
        self.options.insert(name.to_string(), value.to_string());
        (name.to_string(), value.to_string())
    }

    pub fn show_options(&self, name: Option<&str>) -> Vec<(String, String)> {
        match name {
            Some(name) => self
                .options
                .get(name)
                .map(|v| vec![(name.to_string(), v.clone())])
                .unwrap_or_default(),
            None => self
                .options
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    pub fn set_environment(&mut self, name: &str, value: &str) -> (String, String) {
        self.environment.insert(name.to_string(), value.to_string());
        (name.to_string(), value.to_string())
    }

    pub fn show_environment(&self, name: Option<&str>) -> Vec<(String, String)> {
        match name {
            Some(name) => self
                .environment
                .get(name)
                .map(|v| vec![(name.to_string(), v.clone())])
                .unwrap_or_default(),
            None => self
                .environment
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    pub fn kill_session(&mut self, name: &str) -> Result<Session, StateError> {
        let index = self
            .sessions
            .iter()
            .position(|s| s.name == name)
            .ok_or_else(|| StateError::SessionNotFound(name.to_string()))?;
        let removed = self.sessions.remove(index);
        if self.attached_session.as_deref() == Some(removed.name.as_str()) {
            self.attached_session = self.sessions.first().map(|s| s.name.clone());
            self.client.attached_session = self.attached_session.clone();
        }
        self.locked_sessions.remove(&removed.name);
        Ok(removed)
    }

    pub fn kill_server(&mut self) -> usize {
        let count = self.sessions.len();
        self.sessions.clear();
        self.attached_session = None;
        self.client.attached_session = None;
        self.locked_sessions.clear();
        self.server_locked = false;
        self.client.locked = false;
        self.client.suspended = false;
        self.options.clear();
        self.environment.clear();
        self.message_log.clear();
        count
    }

    pub fn start_server(&mut self) -> bool {
        true
    }

    pub fn create_window(
        &mut self,
        session_name: Option<&str>,
        window_name: Option<String>,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];

        let pane = Pane {
            id: self.next_pane_id,
            title: "shell".to_string(),
            last_input: None,
            last_output: None,
            rows: 0,
            cols: 0,
            floating: false,
        };
        self.next_pane_id += 1;

        let window = Window {
            id: self.next_window_id,
            name: window_name.unwrap_or_else(|| session.windows.len().to_string()),
            active_pane_id: pane.id,
            last_pane_id: None,
            layout: WindowLayout::Horizontal,
            layout_tree: Some(SplitNode::Leaf { pane_id: pane.id }),
            current_layout: PresetLayout::EvenHorizontal,
            panes: vec![pane],
        };
        self.next_window_id += 1;

        set_active_window(session, window.id);
        session.windows.push(window.clone());
        Ok((session.clone(), window))
    }

    pub fn has_session(&self, name: &str) -> bool {
        self.sessions.iter().any(|s| s.name == name)
    }

    pub fn rename_session(
        &mut self,
        session_name: &str,
        new_name: &str,
    ) -> Result<Session, StateError> {
        if self.sessions.iter().any(|s| s.name == new_name) {
            return Err(StateError::SessionExists(new_name.to_string()));
        }
        let session_index = self.find_session_index(Some(session_name))?;
        self.sessions[session_index].name = new_name.to_string();
        if self.attached_session.as_deref() == Some(session_name) {
            self.attached_session = Some(new_name.to_string());
            self.client.attached_session = Some(new_name.to_string());
        }
        if self.locked_sessions.remove(session_name) {
            self.locked_sessions.insert(new_name.to_string());
        }
        Ok(self.sessions[session_index].clone())
    }

    pub fn rename_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        new_name: &str,
    ) -> Result<(Session, Window, String), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let old_name = session.windows[window_index].name.clone();
        session.windows[window_index].name = new_name.to_string();
        Ok((
            session.clone(),
            session.windows[window_index].clone(),
            old_name,
        ))
    }

    pub fn kill_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        if session.windows.is_empty() {
            return Err(StateError::NoWindows(session.name.clone()));
        }
        if session.windows.len() == 1 {
            return Err(StateError::LastWindowInSession(session.name.clone()));
        }

        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let removed = session.windows.remove(window_index);

        if session.active_window_id == removed.id {
            set_active_window(session, session.windows[0].id);
        }

        Ok((session.clone(), removed))
    }

    pub fn select_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: &str,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, Some(window_selector), false)?;

        let window = session.windows[window_index].clone();
        set_active_window(session, window.id);
        Ok((session.clone(), window))
    }

    pub fn select_next_window(
        &mut self,
        session_name: Option<&str>,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        if session.windows.is_empty() {
            return Err(StateError::NoWindows(session.name.clone()));
        }
        let current = find_window_index_in_session(session, None, true)?;
        let next = (current + 1) % session.windows.len();
        let window_id = session.windows[next].id;
        set_active_window(session, window_id);
        Ok((session.clone(), session.windows[next].clone()))
    }

    pub fn select_previous_window(
        &mut self,
        session_name: Option<&str>,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        if session.windows.is_empty() {
            return Err(StateError::NoWindows(session.name.clone()));
        }
        let current = find_window_index_in_session(session, None, true)?;
        let prev = if current == 0 {
            session.windows.len() - 1
        } else {
            current - 1
        };
        let window_id = session.windows[prev].id;
        set_active_window(session, window_id);
        Ok((session.clone(), session.windows[prev].clone()))
    }

    pub fn select_last_window(
        &mut self,
        session_name: Option<&str>,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let target = session.last_window_id.unwrap_or(session.active_window_id);
        let idx = session
            .windows
            .iter()
            .position(|w| w.id == target)
            .ok_or_else(|| StateError::WindowNotFound(target.to_string()))?;
        set_active_window(session, target);
        Ok((session.clone(), session.windows[idx].clone()))
    }

    pub fn split_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        title: Option<String>,
        layout: WindowLayout,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];

        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let pane = Pane {
            id: self.next_pane_id,
            title: title.unwrap_or_else(|| "shell".to_string()),
            last_input: None,
            last_output: None,
            rows: 0,
            cols: 0,
            floating: false,
        };
        self.next_pane_id += 1;

        let window_snapshot = {
            let window = &mut session.windows[window_index];
            let target_pane = window.active_pane_id;
            window.panes.push(pane.clone());
            window.layout = layout;
            if let Some(tree) = window.layout_tree.as_mut() {
                let _ = split_leaf(tree, target_pane, pane.id, layout);
            } else {
                window.layout_tree = Some(SplitNode::Split {
                    orientation: layout,
                    split_ratio: default_split_ratio(),
                    first: Box::new(SplitNode::Leaf {
                        pane_id: target_pane,
                    }),
                    second: Box::new(SplitNode::Leaf { pane_id: pane.id }),
                });
            }
            set_active_pane(window, pane.id);
            window.clone()
        };

        set_active_window(session, window_snapshot.id);
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn resize_pane(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
        direction: ResizeDirection,
        amount: u16,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;
        let pane_id = window.panes[pane_index].id;

        let delta = amount.max(1) as i32;
        let total_rows = window
            .panes
            .iter()
            .map(|p| p.rows as usize)
            .max()
            .unwrap_or(40)
            .max(1);
        let total_cols = window
            .panes
            .iter()
            .map(|p| p.cols as usize)
            .max()
            .unwrap_or(120)
            .max(1);
        let mut tree_resized = false;
        if let Some(tree) = window.layout_tree.as_mut() {
            tree_resized = resize_split_for_pane(
                tree,
                pane_id,
                direction,
                delta as u16,
                total_cols,
                total_rows,
            );
        }
        if !tree_resized {
            match direction {
                ResizeDirection::Up | ResizeDirection::Down => {
                    let pane_count = window.panes.len();
                    if pane_count <= 1 {
                        let pane = window.panes[pane_index].clone();
                        let window_clone = window.clone();
                        return Ok((session.clone(), window_clone, pane));
                    }
                    // fallback sizing hints for non-tree layouts
                    let base = window.panes[pane_index].rows.max(10) as i32;
                    let new_rows = match direction {
                        ResizeDirection::Up => (base - delta).max(3),
                        ResizeDirection::Down => (base + delta).max(3),
                        _ => base,
                    };
                    window.panes[pane_index].rows = new_rows as u16;
                }
                ResizeDirection::Left | ResizeDirection::Right => {
                    let base = window.panes[pane_index].cols.max(20) as i32;
                    let new_cols = match direction {
                        ResizeDirection::Left => (base - delta).max(10),
                        ResizeDirection::Right => (base + delta).max(10),
                        _ => base,
                    };
                    window.panes[pane_index].cols = new_cols as u16;
                }
            }
        }

        let pane = window.panes[pane_index].clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn resize_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        rows: u16,
        cols: u16,
    ) -> Result<(Session, Window, Vec<Pane>), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];

        let rows = rows.max(1);
        let cols = cols.max(1);
        for pane in &mut window.panes {
            pane.rows = rows;
            pane.cols = cols;
        }

        let panes = window.panes.clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, panes))
    }

    pub fn swap_panes(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        src_pane_selector: Option<&str>,
        dst_pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];

        if window.panes.len() <= 1 {
            let pane = window
                .panes
                .first()
                .cloned()
                .ok_or_else(|| StateError::NoPanes(window.name.clone()))?;
            let window_snapshot = window.clone();
            let session_snapshot = session.clone();
            return Ok((session_snapshot, window_snapshot, pane));
        }

        let src_index = find_pane_index_in_window(window, src_pane_selector, true)?;
        let dst_index = if let Some(selector) = dst_pane_selector {
            find_pane_index_in_window(window, Some(selector), false)?
        } else {
            (src_index + 1) % window.panes.len()
        };

        if src_index != dst_index {
            let src_id = window.panes[src_index].id;
            let dst_id = window.panes[dst_index].id;
            if let Some(tree) = window.layout_tree.as_mut() {
                let _ = swap_leaf_panes(tree, src_id, dst_id);
            }
        }

        let active = window
            .panes
            .iter()
            .find(|pane| pane.id == window.active_pane_id)
            .cloned()
            .or_else(|| window.panes.first().cloned())
            .ok_or_else(|| StateError::NoPanes(window.name.clone()))?;
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, active))
    }

    pub fn rotate_window(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        upward: bool,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];

        if window.panes.len() > 1 {
            if let Some(tree) = window.layout_tree.as_mut() {
                let mut leaves = Vec::new();
                collect_leaf_ids(tree, &mut leaves);
                if leaves.len() > 1 {
                    if upward {
                        leaves.rotate_left(1);
                    } else {
                        leaves.rotate_right(1);
                    }
                    let mut iter = leaves.into_iter();
                    let _ = assign_leaf_ids(tree, &mut iter);
                }
            } else if upward {
                window.panes.rotate_left(1);
            } else {
                window.panes.rotate_right(1);
            }
        }

        let active = window
            .panes
            .iter()
            .find(|pane| pane.id == window.active_pane_id)
            .cloned()
            .or_else(|| window.panes.first().cloned())
            .ok_or_else(|| StateError::NoPanes(window.name.clone()))?;
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, active))
    }

    pub fn break_pane(
        &mut self,
        session_name: Option<&str>,
        src_window_selector: Option<&str>,
        src_pane_selector: Option<&str>,
        new_window_name: Option<String>,
    ) -> Result<(Session, Window, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let src_window_index = find_window_index_in_session(session, src_window_selector, true)?;
        let src_window = &mut session.windows[src_window_index];

        if src_window.panes.len() == 1 {
            return Err(StateError::LastPaneInWindow(src_window.name.clone()));
        }

        let src_pane_index = find_pane_index_in_window(src_window, src_pane_selector, true)?;
        let pane = src_window.panes.remove(src_pane_index);
        if let Some(tree) = src_window.layout_tree.take() {
            src_window.layout_tree = remove_leaf(tree, pane.id);
        }
        if src_window.active_pane_id == pane.id {
            set_active_pane(src_window, src_window.panes[0].id);
        }
        let src_snapshot = src_window.clone();

        let new_window = Window {
            id: self.next_window_id,
            name: new_window_name.unwrap_or_else(|| session.windows.len().to_string()),
            active_pane_id: pane.id,
            last_pane_id: None,
            layout: WindowLayout::Horizontal,
            layout_tree: Some(SplitNode::Leaf { pane_id: pane.id }),
            current_layout: PresetLayout::EvenHorizontal,
            panes: vec![pane.clone()],
        };
        self.next_window_id += 1;
        session.windows.push(new_window.clone());
        set_active_window(session, new_window.id);

        let dst_snapshot = new_window;
        let session_snapshot = session.clone();
        Ok((session_snapshot, src_snapshot, dst_snapshot, pane))
    }

    pub fn join_pane(
        &mut self,
        session_name: Option<&str>,
        src_window_selector: Option<&str>,
        src_pane_selector: Option<&str>,
        dst_window_selector: Option<&str>,
        dst_pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];

        let src_window_index = find_window_index_in_session(session, src_window_selector, true)?;
        let dst_window_index = find_window_index_in_session(session, dst_window_selector, true)?;
        if src_window_index == dst_window_index {
            return Err(StateError::WindowNotFound(
                "join-pane source and destination must differ".to_string(),
            ));
        }

        let (src_snapshot, dst_snapshot, pane) = if src_window_index < dst_window_index {
            let (left, right) = session.windows.split_at_mut(dst_window_index);
            let src_window = &mut left[src_window_index];
            let dst_window = &mut right[0];
            let pane_index = find_pane_index_in_window(src_window, src_pane_selector, true)?;
            if src_window.panes.len() == 1 {
                return Err(StateError::LastPaneInWindow(src_window.name.clone()));
            }
            let pane = src_window.panes.remove(pane_index);
            if let Some(tree) = src_window.layout_tree.take() {
                src_window.layout_tree = remove_leaf(tree, pane.id);
            }
            if src_window.active_pane_id == pane.id {
                set_active_pane(src_window, src_window.panes[0].id);
            }

            let insert_index = if let Some(selector) = dst_pane_selector {
                find_pane_index_in_window(dst_window, Some(selector), false)?
            } else {
                dst_window.panes.len()
            };
            let target_pane_id = if let Some(selector) = dst_pane_selector {
                let idx = find_pane_index_in_window(dst_window, Some(selector), false)?;
                dst_window.panes[idx].id
            } else {
                dst_window.active_pane_id
            };
            let insert_index = insert_index.min(dst_window.panes.len());
            dst_window.panes.insert(insert_index, pane.clone());
            if let Some(tree) = dst_window.layout_tree.as_mut() {
                let _ = split_leaf(tree, target_pane_id, pane.id, dst_window.layout);
            } else {
                dst_window.layout_tree = Some(SplitNode::Split {
                    orientation: dst_window.layout,
                    split_ratio: default_split_ratio(),
                    first: Box::new(SplitNode::Leaf {
                        pane_id: target_pane_id,
                    }),
                    second: Box::new(SplitNode::Leaf { pane_id: pane.id }),
                });
            }
            set_active_pane(dst_window, pane.id);

            (src_window.clone(), dst_window.clone(), pane)
        } else {
            let (left, right) = session.windows.split_at_mut(src_window_index);
            let dst_window = &mut left[dst_window_index];
            let src_window = &mut right[0];
            let pane_index = find_pane_index_in_window(src_window, src_pane_selector, true)?;
            if src_window.panes.len() == 1 {
                return Err(StateError::LastPaneInWindow(src_window.name.clone()));
            }
            let pane = src_window.panes.remove(pane_index);
            if let Some(tree) = src_window.layout_tree.take() {
                src_window.layout_tree = remove_leaf(tree, pane.id);
            }
            if src_window.active_pane_id == pane.id {
                set_active_pane(src_window, src_window.panes[0].id);
            }

            let insert_index = if let Some(selector) = dst_pane_selector {
                find_pane_index_in_window(dst_window, Some(selector), false)?
            } else {
                dst_window.panes.len()
            };
            let target_pane_id = if let Some(selector) = dst_pane_selector {
                let idx = find_pane_index_in_window(dst_window, Some(selector), false)?;
                dst_window.panes[idx].id
            } else {
                dst_window.active_pane_id
            };
            let insert_index = insert_index.min(dst_window.panes.len());
            dst_window.panes.insert(insert_index, pane.clone());
            if let Some(tree) = dst_window.layout_tree.as_mut() {
                let _ = split_leaf(tree, target_pane_id, pane.id, dst_window.layout);
            } else {
                dst_window.layout_tree = Some(SplitNode::Split {
                    orientation: dst_window.layout,
                    split_ratio: default_split_ratio(),
                    first: Box::new(SplitNode::Leaf {
                        pane_id: target_pane_id,
                    }),
                    second: Box::new(SplitNode::Leaf { pane_id: pane.id }),
                });
            }
            set_active_pane(dst_window, pane.id);

            (src_window.clone(), dst_window.clone(), pane)
        };

        set_active_window(session, dst_snapshot.id);
        let session_snapshot = session.clone();
        Ok((session_snapshot, src_snapshot, dst_snapshot, pane))
    }

    pub fn apply_layout(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        layout: PresetLayout,
    ) -> Result<(Session, Window), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];

        let pane_ids: Vec<u64> = window.panes.iter().map(|p| p.id).collect();
        if pane_ids.is_empty() {
            return Err(StateError::NoPanes(window.name.clone()));
        }

        let (orientation, tree) = build_preset_layout(layout, &pane_ids);
        window.layout = orientation;
        window.layout_tree = Some(tree);
        for pane in window.panes.iter_mut() {
            pane.rows = 0;
            pane.cols = 0;
        }
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot))
    }

    pub fn cycle_layout(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        forward: bool,
    ) -> Result<(Session, Window, PresetLayout), StateError> {
        let presets = [
            PresetLayout::EvenHorizontal,
            PresetLayout::EvenVertical,
            PresetLayout::MainHorizontal,
            PresetLayout::MainVertical,
            PresetLayout::Tiled,
        ];
        let session_index = self.find_session_index(session_name)?;
        let session = &self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let cur = session.windows[window_index].current_layout;
        let idx = presets.iter().position(|p| *p == cur).unwrap_or(0);
        let next = if forward {
            presets[(idx + 1) % presets.len()]
        } else {
            presets[(idx + presets.len() - 1) % presets.len()]
        };
        let (session, _w) = self.apply_layout(session_name, window_selector, next)?;
        let session_index = self.find_session_index(Some(&session.name))?;
        let window_index = find_window_index_in_session(&self.sessions[session_index], window_selector, true)?;
        self.sessions[session_index].windows[window_index].current_layout = next;
        Ok((self.sessions[session_index].clone(), self.sessions[session_index].windows[window_index].clone(), next))
    }

    pub fn clear_pane_history(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;
        window.panes[pane_index].last_output = None;
        let pane = window.panes[pane_index].clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn kill_pane(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;

        let window = &mut session.windows[window_index];
        if window.panes.len() == 1 {
            return Err(StateError::LastPaneInWindow(window.name.clone()));
        }

        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;
        let removed = window.panes.remove(pane_index);
        if let Some(tree) = window.layout_tree.take() {
            window.layout_tree = remove_leaf(tree, removed.id);
        }
        if window.active_pane_id == removed.id {
            set_active_pane(window, window.panes[0].id);
        }

        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, removed))
    }

    pub fn select_pane(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: &str,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;

        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, Some(pane_selector), false)?;
        let pane = window.panes[pane_index].clone();
        set_active_pane(window, pane.id);

        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn select_last_pane(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];

        let target = window.last_pane_id.unwrap_or(window.active_pane_id);
        let pane_index = window
            .panes
            .iter()
            .position(|p| p.id == target)
            .ok_or_else(|| StateError::PaneNotFound(target.to_string()))?;
        let pane = window.panes[pane_index].clone();
        set_active_pane(window, pane.id);

        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn send_keys(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
        keys: &[String],
        literal: bool,
        output: Option<String>,
    ) -> Result<(Session, Window, Pane, String), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;

        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;

        let payload = if literal {
            keys.join("")
        } else {
            keys.join(" ")
        };

        window.panes[pane_index].last_input = Some(payload.clone());
        if let Some(output) = output {
            window.panes[pane_index].last_output = Some(output);
        }
        set_active_pane(window, window.panes[pane_index].id);

        let pane = window.panes[pane_index].clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane, payload))
    }

    pub fn respawn_pane(
        &mut self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
        pane_selector: Option<&str>,
    ) -> Result<(Session, Window, Pane), StateError> {
        let session_index = self.find_session_index(session_name)?;
        let session = &mut self.sessions[session_index];
        let window_index = find_window_index_in_session(session, window_selector, true)?;
        let window = &mut session.windows[window_index];
        let pane_index = find_pane_index_in_window(window, pane_selector, true)?;

        window.panes[pane_index].last_input = None;
        window.panes[pane_index].last_output = None;
        let pane_id = window.panes[pane_index].id;
        set_active_pane(window, pane_id);

        let pane = window.panes[pane_index].clone();
        let window_snapshot = window.clone();
        let session_snapshot = session.clone();
        Ok((session_snapshot, window_snapshot, pane))
    }

    pub fn set_pane_output(&mut self, pane_id: u64, output: String) -> bool {
        for session in &mut self.sessions {
            for window in &mut session.windows {
                for pane in &mut window.panes {
                    if pane.id == pane_id {
                        pane.last_output = Some(output);
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn list_windows(
        &self,
        session_name: Option<&str>,
    ) -> Result<Vec<WindowRecord>, StateError> {
        let sessions = self.session_refs(session_name)?;

        let mut windows = Vec::new();
        for session in sessions {
            for window in &session.windows {
                windows.push(WindowRecord {
                    session_id: session.id,
                    session_name: session.name.clone(),
                    window_id: window.id,
                    window_name: window.name.clone(),
                    pane_count: window.panes.len(),
                    active: session.active_window_id == window.id,
                    layout: window.layout,
                    layout_tree: window.layout_tree.clone(),
                });
            }
        }
        Ok(windows)
    }

    pub fn list_panes(
        &self,
        session_name: Option<&str>,
        window_selector: Option<&str>,
    ) -> Result<Vec<PaneRecord>, StateError> {
        let sessions = self.session_refs(session_name)?;

        let mut panes = Vec::new();
        for session in sessions {
            let window_indexes: Vec<usize> = if let Some(selector) = window_selector {
                vec![find_window_index_in_session(
                    session,
                    Some(selector),
                    false,
                )?]
            } else {
                (0..session.windows.len()).collect()
            };

            for idx in window_indexes {
                let window = &session.windows[idx];
                for pane in &window.panes {
                    panes.push(PaneRecord {
                        session_id: session.id,
                        session_name: session.name.clone(),
                        window_id: window.id,
                        window_name: window.name.clone(),
                        pane_id: pane.id,
                        pane_title: pane.title.clone(),
                        active: window.active_pane_id == pane.id,
                        last_input: pane.last_input.clone(),
                        last_output: pane.last_output.clone(),
                        rows: pane.rows,
                        cols: pane.cols,
                        floating: pane.floating,
                    });
                }
            }
        }
        Ok(panes)
    }

    fn find_session_index(&self, session_name: Option<&str>) -> Result<usize, StateError> {
        match session_name {
            Some(name) => self
                .sessions
                .iter()
                .position(|s| s.name == name)
                .ok_or_else(|| StateError::SessionNotFound(name.to_string())),
            None => {
                if self.sessions.is_empty() {
                    Err(StateError::NoSessions)
                } else {
                    Ok(0)
                }
            }
        }
    }

    fn session_refs(&self, session_name: Option<&str>) -> Result<Vec<&Session>, StateError> {
        if let Some(name) = session_name {
            Ok(vec![self
                .sessions
                .iter()
                .find(|s| s.name == name)
                .ok_or_else(|| {
                    StateError::SessionNotFound(name.to_string())
                })?])
        } else if self.sessions.is_empty() {
            Err(StateError::NoSessions)
        } else {
            Ok(self.sessions.iter().collect())
        }
    }

    fn push_message(&mut self, message: String) {
        self.message_log.push(message);
        if self.message_log.len() > 200 {
            let overflow = self.message_log.len() - 200;
            self.message_log.drain(0..overflow);
        }
    }
}

fn split_leaf(
    node: &mut SplitNode,
    target_pane_id: u64,
    new_pane_id: u64,
    orientation: WindowLayout,
) -> bool {
    match node {
        SplitNode::Leaf { pane_id } if *pane_id == target_pane_id => {
            *node = SplitNode::Split {
                orientation,
                split_ratio: default_split_ratio(),
                first: Box::new(SplitNode::Leaf {
                    pane_id: target_pane_id,
                }),
                second: Box::new(SplitNode::Leaf {
                    pane_id: new_pane_id,
                }),
            };
            true
        }
        SplitNode::Leaf { .. } => false,
        SplitNode::Split { first, second, .. } => {
            split_leaf(first, target_pane_id, new_pane_id, orientation)
                || split_leaf(second, target_pane_id, new_pane_id, orientation)
        }
    }
}

fn remove_leaf(node: SplitNode, pane_id: u64) -> Option<SplitNode> {
    match node {
        SplitNode::Leaf { pane_id: id } => {
            if id == pane_id {
                None
            } else {
                Some(SplitNode::Leaf { pane_id: id })
            }
        }
        SplitNode::Split {
            orientation,
            split_ratio,
            first,
            second,
        } => {
            let left = remove_leaf(*first, pane_id);
            let right = remove_leaf(*second, pane_id);
            match (left, right) {
                (None, None) => None,
                (Some(node), None) | (None, Some(node)) => Some(node),
                (Some(left), Some(right)) => Some(SplitNode::Split {
                    orientation,
                    split_ratio,
                    first: Box::new(left),
                    second: Box::new(right),
                }),
            }
        }
    }
}

fn swap_leaf_panes(node: &mut SplitNode, pane_a: u64, pane_b: u64) -> bool {
    let mut found_a = false;
    let mut found_b = false;
    fn walk(
        node: &mut SplitNode,
        pane_a: u64,
        pane_b: u64,
        found_a: &mut bool,
        found_b: &mut bool,
    ) {
        match node {
            SplitNode::Leaf { pane_id } => {
                if *pane_id == pane_a {
                    *pane_id = pane_b;
                    *found_a = true;
                } else if *pane_id == pane_b {
                    *pane_id = pane_a;
                    *found_b = true;
                }
            }
            SplitNode::Split { first, second, .. } => {
                walk(first, pane_a, pane_b, found_a, found_b);
                walk(second, pane_a, pane_b, found_a, found_b);
            }
        }
    }
    walk(node, pane_a, pane_b, &mut found_a, &mut found_b);
    found_a && found_b
}

fn collect_leaf_ids(node: &SplitNode, out: &mut Vec<u64>) {
    match node {
        SplitNode::Leaf { pane_id } => out.push(*pane_id),
        SplitNode::Split { first, second, .. } => {
            collect_leaf_ids(first, out);
            collect_leaf_ids(second, out);
        }
    }
}

fn assign_leaf_ids(node: &mut SplitNode, ids: &mut impl Iterator<Item = u64>) -> bool {
    match node {
        SplitNode::Leaf { pane_id } => {
            if let Some(next) = ids.next() {
                *pane_id = next;
                true
            } else {
                false
            }
        }
        SplitNode::Split { first, second, .. } => {
            assign_leaf_ids(first, ids) && assign_leaf_ids(second, ids)
        }
    }
}

fn tree_contains_pane(node: &SplitNode, pane_id: u64) -> bool {
    match node {
        SplitNode::Leaf { pane_id: id } => *id == pane_id,
        SplitNode::Split { first, second, .. } => {
            tree_contains_pane(first, pane_id) || tree_contains_pane(second, pane_id)
        }
    }
}

fn resize_split_for_pane(
    node: &mut SplitNode,
    pane_id: u64,
    direction: ResizeDirection,
    amount: u16,
    width: usize,
    height: usize,
) -> bool {
    match node {
        SplitNode::Leaf { .. } => false,
        SplitNode::Split {
            orientation,
            split_ratio,
            first,
            second,
        } => {
            let in_first = tree_contains_pane(first, pane_id);
            let in_second = tree_contains_pane(second, pane_id);
            if !in_first && !in_second {
                return false;
            }

            let ratio = (*split_ratio).clamp(50, 950) as usize;
            let (first_w, second_w, first_h, second_h, axis_total) = match orientation {
                WindowLayout::Vertical => {
                    if width <= 2 {
                        return false;
                    }
                    let axis = width.saturating_sub(1).max(1);
                    let mut left = axis.saturating_mul(ratio) / 1000;
                    left = left.clamp(1, axis.saturating_sub(1).max(1));
                    let right = axis.saturating_sub(left).max(1);
                    (left, right, height, height, axis)
                }
                WindowLayout::Horizontal => {
                    if height <= 2 {
                        return false;
                    }
                    let axis = height.saturating_sub(1).max(1);
                    let mut top = axis.saturating_mul(ratio) / 1000;
                    top = top.clamp(1, axis.saturating_sub(1).max(1));
                    let bottom = axis.saturating_sub(top).max(1);
                    (width, width, top, bottom, axis)
                }
            };

            if in_first
                && resize_split_for_pane(first, pane_id, direction, amount, first_w, first_h)
            {
                return true;
            }
            if in_second
                && resize_split_for_pane(second, pane_id, direction, amount, second_w, second_h)
            {
                return true;
            }

            let wants_vertical =
                matches!(direction, ResizeDirection::Left | ResizeDirection::Right);
            let is_vertical = *orientation == WindowLayout::Vertical;
            if wants_vertical != is_vertical {
                return false;
            }

            let mut ratio = *split_ratio as i32;
            let axis_size = axis_total.max(1);
            let step = ((amount.max(1) as usize) * 1000 / axis_size).max(1) as i32;
            let signed = match direction {
                ResizeDirection::Left | ResizeDirection::Up => -step,
                ResizeDirection::Right | ResizeDirection::Down => step,
            };
            ratio += signed;

            let min_first = min_axis_for_node(first, *orientation) as i32;
            let min_second = min_axis_for_node(second, *orientation) as i32;
            let axis_i32 = axis_size as i32;
            let min_ratio = (min_first * 1000 / axis_i32).clamp(1, 999);
            let max_ratio = (1000 - (min_second * 1000 / axis_i32)).clamp(1, 999);
            if min_ratio > max_ratio {
                return false;
            }
            *split_ratio = ratio.clamp(min_ratio, max_ratio) as u16;
            true
        }
    }
}

fn min_axis_for_node(node: &SplitNode, axis: WindowLayout) -> usize {
    match node {
        SplitNode::Leaf { .. } => MIN_PANE_AXIS_SIZE,
        SplitNode::Split {
            orientation,
            first,
            second,
            ..
        } => {
            let a = min_axis_for_node(first, axis);
            let b = min_axis_for_node(second, axis);
            if *orientation == axis {
                a.saturating_add(b).saturating_add(1)
            } else {
                a.max(b)
            }
        }
    }
}

fn find_window_index_in_session(
    session: &Session,
    window_selector: Option<&str>,
    default_active: bool,
) -> Result<usize, StateError> {
    if session.windows.is_empty() {
        return Err(StateError::NoWindows(session.name.clone()));
    }

    if let Some(selector) = window_selector {
        if let Some(index) = session.windows.iter().position(|w| w.name == selector) {
            return Ok(index);
        }
        if let Ok(window_id) = selector.parse::<u64>() {
            if let Some(index) = session.windows.iter().position(|w| w.id == window_id) {
                return Ok(index);
            }
            let idx = window_id as usize;
            if idx < session.windows.len() {
                return Ok(idx);
            }
        }
        return Err(StateError::WindowNotFound(selector.to_string()));
    }

    if default_active {
        if let Some(index) = session
            .windows
            .iter()
            .position(|w| w.id == session.active_window_id)
        {
            return Ok(index);
        }
    }

    Ok(0)
}

fn find_pane_index_in_window(
    window: &Window,
    pane_selector: Option<&str>,
    default_active: bool,
) -> Result<usize, StateError> {
    if window.panes.is_empty() {
        return Err(StateError::NoPanes(window.name.clone()));
    }

    if let Some(selector) = pane_selector {
        if let Some(index) = window.panes.iter().position(|p| p.title == selector) {
            return Ok(index);
        }
        if let Ok(pane_id) = selector.parse::<u64>() {
            if let Some(index) = window.panes.iter().position(|p| p.id == pane_id) {
                return Ok(index);
            }
            let idx = pane_id as usize;
            if idx < window.panes.len() {
                return Ok(idx);
            }
        }
        return Err(StateError::PaneNotFound(selector.to_string()));
    }

    if default_active {
        if let Some(index) = window
            .panes
            .iter()
            .position(|p| p.id == window.active_pane_id)
        {
            return Ok(index);
        }
    }

    Ok(0)
}

fn set_active_window(session: &mut Session, new_window_id: u64) {
    if session.active_window_id != new_window_id {
        session.last_window_id = Some(session.active_window_id);
    }
    session.active_window_id = new_window_id;
}

fn set_active_pane(window: &mut Window, new_pane_id: u64) {
    if window.active_pane_id != new_pane_id {
        window.last_pane_id = Some(window.active_pane_id);
    }
    window.active_pane_id = new_pane_id;
}

#[derive(Debug, Clone)]
pub struct WindowRecord {
    pub session_id: u64,
    pub session_name: String,
    pub window_id: u64,
    pub window_name: String,
    pub pane_count: usize,
    pub active: bool,
    pub layout: WindowLayout,
    pub layout_tree: Option<SplitNode>,
}

#[derive(Debug, Clone)]
pub struct PaneRecord {
    pub session_id: u64,
    pub session_name: String,
    pub window_id: u64,
    pub window_name: String,
    pub pane_id: u64,
    pub pane_title: String,
    pub active: bool,
    pub last_input: Option<String>,
    pub last_output: Option<String>,
    pub rows: u16,
    pub cols: u16,
    pub floating: bool,
}

#[derive(Debug, Clone)]
pub struct ClientRecord {
    pub id: u64,
    pub name: String,
    pub attached_session: Option<String>,
    pub locked: bool,
    pub suspended: bool,
    pub server_locked: bool,
}
