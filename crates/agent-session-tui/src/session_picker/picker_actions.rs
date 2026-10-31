/// Keyboard action understood by the sessions picker model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionsPickerKey {
    #[cfg(test)]
    MoveDown,
    #[cfg(test)]
    MoveUp,
    #[cfg(test)]
    PageDown,
    #[cfg(test)]
    PageUp,
    #[cfg(test)]
    MoveFirst,
    #[cfg(test)]
    MoveLast,
    CycleRoot,
    CycleRuntimeView,
    CycleSort,
    #[cfg(test)]
    ToggleHelp,
    #[cfg(test)]
    SearchChar(char),
    #[cfg(test)]
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
        agent_session_inventory::sessions::claude_terminal_launch::ClaudeTerminalMode,
    ),
    TerminalTooNarrow,
}
