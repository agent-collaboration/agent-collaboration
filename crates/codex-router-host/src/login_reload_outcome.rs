//! Secret-free observations of the managed app-server's login cache.
use serde::{Deserialize, Serialize};

/// Last completed auth-file observation in this Host lifetime.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LoginReloadOutcome {
    /// The app-server now serves the access token read from the changed file.
    Reloaded,
    /// Upstream has no cached account to which it can apply a guarded reload.
    RefusedFirstLogin,
    /// Upstream retained its existing account instead of adopting another one.
    RefusedDifferentAccount,
    /// The observation could not establish adoption; no automatic retry follows.
    Failed {
        /// Bounded local diagnostic; never contains file contents or backend text.
        reason: String,
    },
}

impl LoginReloadOutcome {
    /// Operator-facing description without account identifiers or credentials.
    #[must_use]
    pub fn description(&self) -> &str {
        match self {
            Self::Reloaded => "reloaded",
            Self::RefusedFirstLogin => {
                "refused-first-login; run codex-router host app-server restart"
            }
            Self::RefusedDifferentAccount => {
                "refused-different-account; run codex-router host app-server restart"
            }
            Self::Failed { reason } => reason,
        }
    }

    pub(crate) fn failed(reason: &str) -> Self {
        Self::Failed {
            reason: reason.to_owned(),
        }
    }
}
