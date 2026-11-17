//! A connection's session runs, as its event sink reads them (D1 step 6).
//! The sink keeps no generations: it asks the connection's session cells,
//! which decide each publication's run claim under the cell's lock.

use crate::session_event_sink::{EventSinkClosed, GenerationPublication};
use provider_session_machines::SessionGeneration;
use provider_session_machines::session_run_claims::RunClaim;

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
