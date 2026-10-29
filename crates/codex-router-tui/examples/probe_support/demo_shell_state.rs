use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineChoice {
    Sunclaw,
    Sunbook,
    BuildBox,
}
impl MachineChoice {
    pub const ALL: [Self; 3] = [Self::Sunclaw, Self::Sunbook, Self::BuildBox];
    pub fn label(self) -> &'static str {
        match self {
            Self::Sunclaw => "Sunclaw",
            Self::Sunbook => "Sunbook",
            Self::BuildBox => "Build box",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogFocus {
    Machine,
    Cancel,
    Start,
}
impl DialogFocus {
    fn advance(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Machine, false) | (Self::Start, true) => Self::Cancel,
            (Self::Cancel, false) | (Self::Machine, true) => Self::Start,
            (Self::Start, false) | (Self::Cancel, true) => Self::Machine,
        }
    }
}
#[derive(Clone, Debug)]
pub enum DemoOverlay {
    Menu {
        selected: usize,
    },
    NewSession {
        focus: DialogFocus,
    },
    MachinePicker {
        query: String,
        selected: MachineChoice,
    },
    Filter {
        query: String,
    },
    Help,
}
#[derive(Clone, Copy, Debug)]
pub struct CellRectangle {
    pub left: u16,
    pub top: u16,
    pub width: u16,
    pub height: u16,
}
impl CellRectangle {
    pub fn contains(self, mouse: &FullscreenMouseEvent) -> bool {
        mouse.column >= self.left
            && mouse.column < self.left + self.width
            && mouse.row >= self.top
            && mouse.row < self.top + self.height
    }
}
pub fn overlay_rectangle(overlay: &DemoOverlay) -> CellRectangle {
    match overlay {
        DemoOverlay::Menu { .. } => CellRectangle {
            left: 2,
            top: 1,
            width: 34,
            height: 10,
        },
        DemoOverlay::NewSession { .. } => CellRectangle {
            left: 17,
            top: 5,
            width: 52,
            height: 12,
        },
        DemoOverlay::MachinePicker { .. } => CellRectangle {
            left: 20,
            top: 9,
            width: 40,
            height: 9,
        },
        DemoOverlay::Filter { .. } => CellRectangle {
            left: 17,
            top: 5,
            width: 52,
            height: 7,
        },
        DemoOverlay::Help => CellRectangle {
            left: 12,
            top: 4,
            width: 66,
            height: 13,
        },
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKey {
    Atlas,
    Compiler,
    Interface,
    Storage,
    Batches,
    Terminal,
}
#[derive(Clone, Copy, Debug)]
pub struct FakeSession {
    pub key: SessionKey,
    pub title: &'static str,
    pub folder: &'static str,
}
pub const FAKE_SESSIONS: [FakeSession; 6] = [
    FakeSession {
        key: SessionKey::Atlas,
        title: "Atlas sketch",
        folder: "/demo/atlas",
    },
    FakeSession {
        key: SessionKey::Compiler,
        title: "Compiler notes",
        folder: "/demo/compiler",
    },
    FakeSession {
        key: SessionKey::Interface,
        title: "UI sandbox",
        folder: "/demo/interface",
    },
    FakeSession {
        key: SessionKey::Storage,
        title: "Storage sketch",
        folder: "/demo/storage",
    },
    FakeSession {
        key: SessionKey::Batches,
        title: "Batch routing",
        folder: "/demo/batches",
    },
    FakeSession {
        key: SessionKey::Terminal,
        title: "Terminal lab",
        folder: "/demo/terminal",
    },
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionSort {
    Updated,
    Name,
}
#[derive(Clone, Debug)]
pub struct DemoShellState {
    pub width: u16,
    pub height: u16,
    pub overlays: Vec<DemoOverlay>,
    pub selected_session: Option<SessionKey>,
    pub selected_machine: MachineChoice,
    pub sort: SessionSort,
    pub this_folder: bool,
    pub filter_query: String,
    pub last_action: String,
    pub quit_requested: bool,
}
impl DemoShellState {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            overlays: Vec::new(),
            selected_session: Some(SessionKey::Atlas),
            selected_machine: MachineChoice::Sunclaw,
            sort: SessionSort::Updated,
            this_folder: false,
            filter_query: String::new(),
            last_action: "Ready · fake sessions only".to_owned(),
            quit_requested: false,
        }
    }
    pub fn visible_sessions(&self) -> Vec<FakeSession> {
        let mut sessions = FAKE_SESSIONS
            .iter()
            .copied()
            .filter(|session| {
                session
                    .title
                    .to_lowercase()
                    .contains(&self.filter_query.to_lowercase())
                    && (!self.this_folder || session.folder == "/demo/atlas")
            })
            .collect::<Vec<_>>();
        if self.sort == SessionSort::Name {
            sessions.sort_by_key(|session| session.title);
        }
        sessions
    }
    fn reconcile_selection(&mut self) {
        let sessions = self.visible_sessions();
        if !sessions
            .iter()
            .any(|session| Some(session.key) == self.selected_session)
        {
            self.selected_session = sessions.first().map(|session| session.key);
        }
    }
    fn move_session(&mut self, down: bool) {
        let sessions = self.visible_sessions();
        let current = sessions
            .iter()
            .position(|session| Some(session.key) == self.selected_session)
            .unwrap_or(0);
        let next = if down {
            current
                .saturating_add(1)
                .min(sessions.len().saturating_sub(1))
        } else {
            current.saturating_sub(1)
        };
        self.selected_session = sessions.get(next).map(|session| session.key);
    }
    fn open_session(&mut self) {
        if let Some(session) = self
            .visible_sessions()
            .iter()
            .find(|session| Some(session.key) == self.selected_session)
        {
            self.last_action = format!("Opened {}", session.title);
        }
    }
    pub fn matching_machines(query: &str) -> Vec<MachineChoice> {
        MachineChoice::ALL
            .into_iter()
            .filter(|machine| {
                machine
                    .label()
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            })
            .collect()
    }
    fn start_session(&mut self) {
        self.last_action = format!("Started on {}", self.selected_machine.label());
        self.overlays.clear();
    }
    fn open_dialog(&mut self) {
        self.overlays.push(DemoOverlay::NewSession {
            focus: DialogFocus::Machine,
        });
        self.last_action = "New session dialog".to_owned();
    }
    fn run_menu_item(&mut self, selected: usize) {
        match selected {
            0 => self.open_dialog(),
            1 => {
                self.overlays.push(DemoOverlay::Filter {
                    query: self.filter_query.clone(),
                });
                self.last_action = "Filter sessions".to_owned();
            }
            2 | 3 => {
                self.sort = if selected == 2 {
                    SessionSort::Updated
                } else {
                    SessionSort::Name
                };
                self.last_action = format!("Sort: {:?}", self.sort);
                self.overlays.pop();
            }
            4 => {
                self.overlays.push(DemoOverlay::Help);
                self.last_action = "Help opened".to_owned();
            }
            _ => {
                self.overlays.pop();
                self.last_action = "Menu closed".to_owned();
            }
        }
    }
    pub fn handle_event(&mut self, event: TerminalEvent) {
        match event {
            TerminalEvent::Resize(width, height) => {
                self.width = width;
                self.height = height;
            }
            TerminalEvent::Key(key) if key.kind == KeyEventKind::Press => {
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.quit_requested = true;
                    return;
                }
                if self.width < 96 || self.height < 24 {
                    return;
                }
                // A single live read per event; an event never falls through after dismissing.
                match self.overlays.last().cloned() {
                    None => self.background_key(key),
                    Some(overlay) => self.overlay_key(overlay, key),
                }
            }
            TerminalEvent::FullscreenMouse(mouse)
                if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) =>
            {
                if self.width < 96 || self.height < 24 {
                    return;
                }
                match self.overlays.last().cloned() {
                    None => self.background_click(mouse),
                    Some(overlay) => self.overlay_click(overlay, mouse),
                }
            }
            TerminalEvent::BracketedPaste(text) => self.append_overlay_query(&text),
            _ => {}
        }
    }
    fn background_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.move_session(false),
            KeyCode::Down => self.move_session(true),
            KeyCode::Enter => self.open_session(),
            KeyCode::F(10) => {
                self.overlays.push(DemoOverlay::Menu { selected: 0 });
                self.last_action = "Menu opened".to_owned();
            }
            KeyCode::F(1) => {
                self.overlays.push(DemoOverlay::Help);
                self.last_action = "Help opened".to_owned();
            }
            KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.open_dialog()
            }
            KeyCode::Char('/') => {
                self.overlays.push(DemoOverlay::Filter {
                    query: self.filter_query.clone(),
                });
                self.last_action = "Filter sessions".to_owned();
            }
            _ => {}
        }
    }
    fn overlay_key(&mut self, overlay: DemoOverlay, key: KeyEvent) {
        if key.code == KeyCode::F(1) && !matches!(overlay, DemoOverlay::Help) {
            self.overlays.push(DemoOverlay::Help);
            return;
        }
        match overlay {
            DemoOverlay::Menu { selected } => match key.code {
                KeyCode::Esc | KeyCode::F(10) => {
                    self.overlays.pop();
                    self.last_action = "Menu closed".to_owned();
                }
                KeyCode::Up | KeyCode::Down => {
                    let next = if key.code == KeyCode::Up {
                        selected.saturating_sub(1)
                    } else {
                        selected.saturating_add(1).min(5)
                    };
                    if let Some(DemoOverlay::Menu { selected }) = self.overlays.last_mut() {
                        *selected = next;
                    }
                }
                KeyCode::Enter => self.run_menu_item(selected),
                KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.open_dialog()
                }
                KeyCode::Char('/') => self.run_menu_item(1),
                _ => {}
            },
            DemoOverlay::NewSession { focus } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "New session cancelled".to_owned();
                }
                KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.start_session()
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    if let Some(DemoOverlay::NewSession { focus: current }) =
                        self.overlays.last_mut()
                    {
                        *current = focus.advance(
                            key.code == KeyCode::BackTab
                                || key.modifiers.contains(KeyModifiers::SHIFT),
                        );
                    }
                }
                KeyCode::Enter => match focus {
                    DialogFocus::Machine => self.overlays.push(DemoOverlay::MachinePicker {
                        query: String::new(),
                        selected: self.selected_machine,
                    }),
                    DialogFocus::Cancel => {
                        self.overlays.pop();
                        self.last_action = "New session cancelled".to_owned();
                    }
                    DialogFocus::Start => self.start_session(),
                },
                _ => {}
            },
            DemoOverlay::MachinePicker { query, selected } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "Machine list closed".to_owned();
                }
                KeyCode::Up | KeyCode::Down => {
                    let options = Self::matching_machines(&query);
                    let current = options
                        .iter()
                        .position(|machine| *machine == selected)
                        .unwrap_or(0);
                    let next = if key.code == KeyCode::Up {
                        current.saturating_sub(1)
                    } else {
                        current
                            .saturating_add(1)
                            .min(options.len().saturating_sub(1))
                    };
                    if let (Some(machine), Some(DemoOverlay::MachinePicker { selected, .. })) =
                        (options.get(next), self.overlays.last_mut())
                    {
                        *selected = *machine;
                    }
                }
                KeyCode::Enter => {
                    if Self::matching_machines(&query).contains(&selected) {
                        self.selected_machine = selected;
                        self.overlays.pop();
                        self.last_action = format!("Machine: {}", selected.label());
                    }
                }
                KeyCode::Backspace => {
                    if let Some(DemoOverlay::MachinePicker { query, selected }) =
                        self.overlays.last_mut()
                    {
                        query.pop();
                        if let Some(first) = Self::matching_machines(query).first() {
                            *selected = *first;
                        }
                    }
                }
                KeyCode::Char(character)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    self.append_overlay_query(&character.to_string())
                }
                _ => {}
            },
            DemoOverlay::Filter { .. } => match key.code {
                KeyCode::Esc => {
                    self.overlays.pop();
                    self.last_action = "Filter cancelled".to_owned();
                }
                KeyCode::Enter => {
                    if let Some(DemoOverlay::Filter { query }) = self.overlays.pop() {
                        self.filter_query = query;
                        self.reconcile_selection();
                        self.last_action = format!("Filtered: {}", self.filter_query);
                    }
                }
                KeyCode::Backspace => {
                    if let Some(DemoOverlay::Filter { query }) = self.overlays.last_mut() {
                        query.pop();
                    }
                }
                KeyCode::Char(character)
                    if !key.modifiers.intersects(
                        KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER,
                    ) =>
                {
                    self.append_overlay_query(&character.to_string())
                }
                _ => {}
            },
            DemoOverlay::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::F(1) | KeyCode::Enter) {
                    self.overlays.pop();
                    self.last_action = "Help closed".to_owned();
                }
            }
        }
    }
    fn append_overlay_query(&mut self, text: &str) {
        let text = text
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>();
        match self.overlays.last_mut() {
            Some(DemoOverlay::MachinePicker { query, selected }) => {
                query.push_str(&text);
                if let Some(first) = Self::matching_machines(query).first() {
                    *selected = *first;
                }
            }
            Some(DemoOverlay::Filter { query }) => query.push_str(&text),
            _ => {}
        }
    }
    fn background_click(&mut self, mouse: FullscreenMouseEvent) {
        if mouse.row == 0 {
            match mouse.column {
                2..=9 => {
                    self.overlays.push(DemoOverlay::Menu { selected: 0 });
                    self.last_action = "Menu opened".to_owned();
                }
                11..=19 => self.open_dialog(),
                21..=33 => {
                    self.overlays.push(DemoOverlay::Filter {
                        query: self.filter_query.clone(),
                    });
                    self.last_action = "Filter sessions".to_owned();
                }
                35..=57 => {
                    self.this_folder = !self.this_folder;
                    self.reconcile_selection();
                    self.last_action = if self.this_folder {
                        "This folder"
                    } else {
                        "All folders"
                    }
                    .to_owned();
                }
                _ => {}
            }
        } else if mouse.row >= 2 && (mouse.row - 2) % 3 < 2 && mouse.column >= 2 {
            let index = usize::from((mouse.row - 2) / 3);
            if let Some(session) = self.visible_sessions().get(index) {
                self.selected_session = Some(session.key);
                self.open_session();
            }
        }
    }
    fn overlay_click(&mut self, overlay: DemoOverlay, mouse: FullscreenMouseEvent) {
        let rect = overlay_rectangle(&overlay);
        // Outside clicks are consumed; only the menu/combo dismiss. Dialogs stay modal.
        if !rect.contains(&mouse) {
            if matches!(
                overlay,
                DemoOverlay::Menu { .. } | DemoOverlay::MachinePicker { .. }
            ) {
                self.overlays.pop();
                self.last_action = "Overlay dismissed".to_owned();
            }
            return;
        }
        let row = mouse.row - rect.top;
        let column = mouse.column - rect.left;
        match overlay {
            DemoOverlay::Menu { .. } if (2..8).contains(&row) => {
                self.run_menu_item(usize::from(row - 2))
            }
            DemoOverlay::NewSession { .. } => {
                if row == 4 && (3..49).contains(&column) {
                    self.overlays.push(DemoOverlay::MachinePicker {
                        query: String::new(),
                        selected: self.selected_machine,
                    });
                } else if row == 9 {
                    if (3..25).contains(&column) {
                        self.overlays.pop();
                        self.last_action = "New session cancelled".to_owned();
                    } else if (28..50).contains(&column) {
                        self.start_session();
                    }
                }
            }
            DemoOverlay::MachinePicker { query, .. } if row >= 3 => {
                if let Some(machine) = Self::matching_machines(&query).get(usize::from(row - 3)) {
                    self.selected_machine = *machine;
                    self.overlays.pop();
                    self.last_action = format!("Machine: {}", machine.label());
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
#[path = "demo_shell_tests.rs"]
mod tests;
