/// Keyboard action understood by the sessions picker model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionsPickerKey {
    MoveDown,
    MoveUp,
    PageDown,
    PageUp,
    MoveFirst,
    MoveLast,
    CycleRoot,
    CycleRuntimeView,
    CycleSort,
    ToggleHelp,
    SearchChar(char),
    SearchBackspace,
    ClearSearch,
}

/// User action selected from the sessions picker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionsPickerOutcome {
    ResumeSession(agent_session_inventory::sessions::SessionActionSelection),
    ForkSession(agent_session_inventory::sessions::SessionActionSelection),
    StartNewSession,
    PreparedSourceSession(
        Box<agent_session_inventory::sessions::source_ssh_protocol::PreparedSourceSession>,
    ),
    InterruptedClientAction(
        Box<agent_session_inventory::sessions::source_client_action::CapturedClientAction>,
    ),
    LaunchNativeClaude(
        Box<agent_session_inventory::sessions::source_client_action::CapturedClientAction>,
    ),
    TerminalTooNarrow,
}
