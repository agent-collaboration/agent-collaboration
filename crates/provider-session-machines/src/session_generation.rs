//! A session's run identity (D1 R14).

/// One run of a session, from the create, load or resume the connection
/// admitted to its Closed (D1 R14). Every per-session publication carries the
/// generation it was produced under; once its run has ended, nothing more is
/// published for it.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SessionGeneration(u64);

impl SessionGeneration {
    /// What a sink that doesn't track generations hands out.
    pub const UNTRACKED: Self = Self(0);

    /// A tracking sink's numbering.
    #[must_use]
    pub const fn from_number(number: u64) -> Self {
        Self(number)
    }

    #[must_use]
    pub const fn number(self) -> u64 {
        self.0
    }
}
