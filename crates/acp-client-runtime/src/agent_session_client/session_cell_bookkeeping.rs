//! The connection's typed session states (D1 state machines, design note
//! §11–§16): one SessionCell per provider session, replacing the connection
//! task's separate sets of pending loads, pending closes, releases and
//! generations. Every lifecycle decision is a transition on that one cell;
//! the connection task runs the ACP calls and actor registration the
//! transitions call for.

use super::ExternalProviderRuntimeError;
use crate::SessionGeneration;
use crate::session_cell_machine::{
    SessionCell, SessionCellEffect, SessionCellEvent, SessionCellMachine, SessionCellRefusal,
};
use crate::session_lifecycle_answers::refusal_answer;
use crate::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleRefusal,
};
use state_machine_core::{StateMachine, TransitionError};
use std::collections::HashMap;

pub(super) type SessionCells = HashMap<String, SessionCell>;

/// Admits a created identity: NotLoaded to Live under the run's next
/// generation. A session already loading, live or closing is a duplicate.
/// `last_published` is the sink's generation for a session this connection
/// hasn't run, so a run here is newer than one on an earlier connection.
pub(super) fn admit_create(
    cells: &mut SessionCells,
    session_id: &str,
    last_published: SessionGeneration,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    seed(cells, session_id, last_published);
    apply(cells, session_id, SessionCellEvent::CreateAdmitted)
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))?;
    live_or_loading_generation(cells, session_id)
}

/// Returns an admitted run whose actor never registered to NotLoaded,
/// keeping its generation as the last one so the next run is newer.
pub(super) fn abandon_run(
    cells: &mut SessionCells,
    session_id: &str,
    generation: SessionGeneration,
) {
    // Stale only when the cell already moved on; nothing is left to undo.
    let _ignored = apply(
        cells,
        session_id,
        SessionCellEvent::ActorExited { generation },
    );
}

/// Admits a load or resume: NotLoaded to Loading under the run's next
/// generation. Loading, live or closing sessions refuse it.
/// `last_published` seeds a session this connection hasn't run, as for a
/// create.
pub(super) fn admit_load(
    cells: &mut SessionCells,
    session_id: &str,
    last_published: SessionGeneration,
    via: RestoreVia,
    attempt: LoadAttempt,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    seed(cells, session_id, last_published);
    apply(
        cells,
        session_id,
        SessionCellEvent::LoadRequested { via, attempt },
    )
    .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))?;
    live_or_loading_generation(cells, session_id)
}

/// The restore admitted as `attempt` is still the session's load: no close
/// or other load has replaced it since.
pub(super) fn is_current_load(
    cells: &SessionCells,
    session_id: &str,
    attempt: LoadAttempt,
) -> bool {
    matches!(
        cells.get(session_id).map(|cell| cell.lifecycle),
        Some(SessionLifecycle::Loading { attempt: current, .. }) if current == attempt
    )
}

/// Settles the load admitted as `attempt`: Live when its actor registered,
/// NotLoaded otherwise. A stale attempt changes nothing.
pub(super) fn settle_load(
    cells: &mut SessionCells,
    session_id: &str,
    attempt: LoadAttempt,
    registered: bool,
) {
    let event = if registered {
        SessionCellEvent::Loaded { attempt }
    } else {
        SessionCellEvent::LoadFailed { attempt }
    };
    // Stale only when a close already replaced this load; the caller checked.
    let _ignored = apply(cells, session_id, event);
}

/// Admits a close: Live or Loading to Closing. Closing refuses it as busy;
/// NotLoaded answers "nothing to close" with its next steps (M8).
pub(super) fn admit_close(
    cells: &mut SessionCells,
    session_id: &str,
    attempt: CloseAttempt,
) -> Result<(), ExternalProviderRuntimeError> {
    apply(
        cells,
        session_id,
        SessionCellEvent::CloseRequested { attempt },
    )
    .map(|_effects| ())
    .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))
}

/// Settles the close admitted as `attempt`. A done close ends NotLoaded and
/// returns the generation its Closed publishes under; a failed close goes
/// back to Live from Live and to NotLoaded from Loading.
pub(super) fn settle_close(
    cells: &mut SessionCells,
    session_id: &str,
    attempt: CloseAttempt,
    freed: bool,
) -> Option<SessionGeneration> {
    let event = if freed {
        SessionCellEvent::CloseDone { attempt }
    } else {
        SessionCellEvent::CloseFailed { attempt }
    };
    apply(cells, session_id, event)
        .ok()
        .and_then(|effects| published_closed_generation(&effects))
}

/// The session is closing on this connection.
pub(super) fn is_closing(cells: &SessionCells, session_id: &str) -> bool {
    matches!(
        cells.get(session_id).map(|cell| cell.lifecycle),
        Some(SessionLifecycle::Closing { .. })
    )
}

/// Starts a cell for a session this connection hasn't run, after the
/// generation its last run published under.
fn seed(cells: &mut SessionCells, session_id: &str, last_published: SessionGeneration) {
    cells
        .entry(session_id.to_owned())
        .or_insert_with(|| SessionCell::not_loaded_after(last_published));
}

/// Applies `event` to the session's cell (a fresh one for a session this
/// connection hasn't run). The cell changes only when the move succeeds.
fn apply(
    cells: &mut SessionCells,
    session_id: &str,
    event: SessionCellEvent,
) -> Result<Vec<SessionCellEffect>, TransitionError<SessionCellRefusal>> {
    let current = cells
        .get(session_id)
        .cloned()
        .unwrap_or_else(SessionCell::fresh);
    let (next, effects) = SessionCellMachine::transition(&current, event)?;
    cells.insert(session_id.to_owned(), next);
    Ok(effects)
}

/// The generation a done close's Closed publishes under.
fn published_closed_generation(effects: &[SessionCellEffect]) -> Option<SessionGeneration> {
    effects.iter().find_map(|effect| match effect {
        SessionCellEffect::Lifecycle(SessionLifecycleEffect::PublishClosed { generation }) => {
            Some(*generation)
        }
        SessionCellEffect::Lifecycle(_)
        | SessionCellEffect::Entry { .. }
        | SessionCellEffect::Turn(_) => None,
    })
}

fn live_or_loading_generation(
    cells: &SessionCells,
    session_id: &str,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    cells
        .get(session_id)
        .and_then(SessionCell::run_generation)
        .ok_or(ExternalProviderRuntimeError::LocalBusy)
}

/// The runtime error for a refused lifecycle move. A busy session keeps
/// today's LocalBusy; "nothing to close" answers with the state and the
/// valid next steps (M8).
fn lifecycle_refusal_error(
    cells: &SessionCells,
    session_id: &str,
    refusal: TransitionError<SessionCellRefusal>,
) -> ExternalProviderRuntimeError {
    let state = cells
        .get(session_id)
        .cloned()
        .unwrap_or_else(SessionCell::fresh)
        .lifecycle;
    match refusal {
        TransitionError::Domain(SessionCellRefusal::Lifecycle(
            refusal @ SessionLifecycleRefusal::NothingToClose,
        )) => ExternalProviderRuntimeError::SessionStateRefused {
            answer: refusal_answer(&state, refusal),
        },
        TransitionError::Domain(SessionCellRefusal::Lifecycle(
            SessionLifecycleRefusal::DuplicateIdentity,
        )) => ExternalProviderRuntimeError::Operation(
            "provider returned a duplicate conversation identity".to_owned(),
        ),
        TransitionError::Domain(
            SessionCellRefusal::Lifecycle(
                SessionLifecycleRefusal::SessionLoading
                | SessionLifecycleRefusal::AlreadyLoaded
                | SessionLifecycleRefusal::SessionClosing
                | SessionLifecycleRefusal::NotLoaded
                | SessionLifecycleRefusal::CloseInProgress,
            )
            | SessionCellRefusal::Line(_)
            | SessionCellRefusal::Entry(_)
            | SessionCellRefusal::Turn(_)
            | SessionCellRefusal::NotHead
            | SessionCellRefusal::UnknownEntry
            | SessionCellRefusal::NotLive,
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => ExternalProviderRuntimeError::LocalBusy,
    }
}

#[cfg(test)]
#[path = "session_cell_bookkeeping_test.rs"]
mod tests;
