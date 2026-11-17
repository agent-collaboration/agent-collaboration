//! Which run a sink publication belongs to, checked against the session's
//! cell (D1 step 6, design note §4). The cell is the one copy of run state:
//! the sink keeps no generation table and asks the cell instead, enqueuing
//! under the cell's lock, so nothing of a run lands after its terminal fact.

use crate::SessionGeneration;
use crate::session_cell_table::SessionCellTable;
use crate::session_event_sink::{EventSinkClosed, GenerationPublication};
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
pub(crate) fn run_admits(lifecycle: &SessionLifecycle, claim: RunClaim) -> bool {
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

/// A connection's session runs, as the sink reads them.
pub trait SessionRuns: Send + Sync {
    /// Runs `enqueue` under the session cell's lock when `claim` holds;
    /// Ended, without running it, when it doesn't. `enqueue` must not block
    /// or await: it pushes onto the sink's unbounded pipe.
    fn publish_if(
        &self,
        session_id: &str,
        claim: RunClaim,
        enqueue: &mut dyn FnMut() -> Result<(), EventSinkClosed>,
    ) -> Result<GenerationPublication, EventSinkClosed>;

    /// The session's run now: its loading, live or closing generation, or
    /// the last one it ended.
    fn current_generation(&self, session_id: &str) -> SessionGeneration;
}

impl SessionRuns for SessionCellTable {
    fn publish_if(
        &self,
        session_id: &str,
        claim: RunClaim,
        enqueue: &mut dyn FnMut() -> Result<(), EventSinkClosed>,
    ) -> Result<GenerationPublication, EventSinkClosed> {
        self.read(session_id, |cell| {
            if run_admits(&cell.lifecycle, claim) {
                enqueue().map(|()| GenerationPublication::Published)
            } else {
                Ok(GenerationPublication::Ended)
            }
        })
    }

    fn current_generation(&self, session_id: &str) -> SessionGeneration {
        self.read(session_id, |cell| match cell.lifecycle {
            SessionLifecycle::NotLoaded { last } => last,
            SessionLifecycle::Loading { generation, .. }
            | SessionLifecycle::Live { generation }
            | SessionLifecycle::Closing { generation, .. } => generation,
        })
    }
}

#[cfg(test)]
#[path = "session_run_claims_test.rs"]
mod tests;
