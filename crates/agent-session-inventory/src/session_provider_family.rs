use collaboration_client::protocol::ProviderKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionProviderFamily {
    Codex,
    External(ProviderKind),
    Unclassified,
}

impl SessionProviderFamily {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Codex => "Codex",
            Self::External(ProviderKind::ClaudeCode) => "Claude Code",
            Self::External(ProviderKind::Cursor) => "Cursor",
            Self::Unclassified => "Unclassified",
        }
    }
}
