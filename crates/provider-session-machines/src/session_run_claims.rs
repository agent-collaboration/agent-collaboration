//! Which run a sink publication belongs to, decided by the session's
//! lifecycle (D1 step 6, design note §4). The cell is the one copy of run
//! state: a sink asks it instead of keeping generations of its own.

use crate::SessionGeneration;
use crate::session_lifecycle_machine::SessionLifecycle;

/// The run a publication claims.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RunClaim {
    /// Whatever run the session has now, for connection-wide publishers
    /// (auth status, provider exit). A session this connection never ran
    /// also takes it, as before; an ended run doesn't.
    Current,
    /// Exactly this run, while it is loading, live or closing.
    Exact(SessionGeneration),
    /// This run's terminal fact (Closed, Unloaded): only once the session
    /// has ended NotLoaded with this run as its last, never for an older one.
    Terminal(SessionGeneration),
}

/// Whether `claim` holds for a session in `lifecycle`.
#[must_use]
pub fn run_admits(lifecycle: &SessionLifecycle, claim: RunClaim) -> bool {
    match (lifecycle, claim) {
        (
            SessionLifecycle::Loading { .. }
            | SessionLifecycle::Live { .. }
            | SessionLifecycle::Closing { .. },
            RunClaim::Current,
        ) => true,
        (SessionLifecycle::NotLoaded { last }, RunClaim::Current) => {
            *last == SessionGeneration::UNTRACKED
        }
        (
            SessionLifecycle::Loading { generation, .. }
            | SessionLifecycle::Live { generation }
            | SessionLifecycle::Closing { generation, .. },
            RunClaim::Exact(claimed),
        ) => *generation == claimed,
        (SessionLifecycle::NotLoaded { .. }, RunClaim::Exact(_)) => false,
        (SessionLifecycle::NotLoaded { last }, RunClaim::Terminal(claimed)) => *last == claimed,
        (
            SessionLifecycle::Loading { .. }
            | SessionLifecycle::Live { .. }
            | SessionLifecycle::Closing { .. },
            RunClaim::Terminal(_),
        ) => false,
    }
}
