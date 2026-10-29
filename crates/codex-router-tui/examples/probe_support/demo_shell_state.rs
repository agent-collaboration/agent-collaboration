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
pub enum ClientChoice {
    Codex,
    Claude,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryChoice {
    Atlas,
    Sandbox,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogChoice {
    Client(ClientChoice),
    Machine(MachineChoice),
    Directory(DirectoryChoice),
}
impl DialogChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Client(ClientChoice::Codex) => "Codex",
            Self::Client(ClientChoice::Claude) => "Claude",
            Self::Machine(machine) => machine.label(),
            Self::Directory(DirectoryChoice::Atlas) => "/demo/atlas",
            Self::Directory(DirectoryChoice::Sandbox) => "/demo/sandbox",
        }
    }
    pub fn field_label(self) -> &'static str {
        match self {
            Self::Client(_) => "Client",
            Self::Machine(_) => "Machine",
            Self::Directory(_) => "Directory",
        }
    }
    pub fn matching(self, query: &str) -> Vec<Self> {
        let choices = match self {
            Self::Client(_) => vec![
                Self::Client(ClientChoice::Codex),
                Self::Client(ClientChoice::Claude),
            ],
            Self::Machine(_) => MachineChoice::ALL.into_iter().map(Self::Machine).collect(),
            Self::Directory(_) => vec![
                Self::Directory(DirectoryChoice::Atlas),
                Self::Directory(DirectoryChoice::Sandbox),
            ],
        };
        choices
            .into_iter()
            .filter(|choice| {
                choice
                    .label()
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            })
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogFocus {
    Client,
    Machine,
    Directory,
    Cancel,
    Start,
}
impl DialogFocus {
    pub fn advance(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Client, false) | (Self::Directory, true) => Self::Machine,
            (Self::Machine, false) | (Self::Cancel, true) => Self::Directory,
            (Self::Directory, false) | (Self::Start, true) => Self::Cancel,
            (Self::Cancel, false) | (Self::Client, true) => Self::Start,
            (Self::Start, false) | (Self::Machine, true) => Self::Client,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShellFocus {
    Menu,
    New,
    Filter,
    Scope,
    List,
}
impl ShellFocus {
    pub fn advance(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Menu, false) | (Self::Filter, true) => Self::New,
            (Self::New, false) | (Self::Scope, true) => Self::Filter,
            (Self::Filter, false) | (Self::List, true) => Self::Scope,
            (Self::Scope, false) | (Self::Menu, true) => Self::List,
            (Self::List, false) | (Self::New, true) => Self::Menu,
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
    ChoicePicker {
        query: String,
        selected: DialogChoice,
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
        DemoOverlay::ChoicePicker { selected, .. } => CellRectangle {
            left: 20,
            top: match selected {
                DialogChoice::Client(_) => 8,
                DialogChoice::Machine(_) => 9,
                DialogChoice::Directory(_) => 10,
            },
            width: 40,
            height: 9,
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
    pub selected_client: ClientChoice,
    pub selected_directory: DirectoryChoice,
    pub focus: ShellFocus,
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
            selected_client: ClientChoice::Codex,
            selected_directory: DirectoryChoice::Atlas,
            focus: ShellFocus::List,
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
}

#[cfg(test)]
#[path = "demo_shell_tests.rs"]
mod tests;
