//! The connection's typed session states (D1 state machines, design note
//! §11–§16): the lifecycle moves on each provider session's `SessionCell`,
//! replacing the connection's separate sets of pending loads, pending closes,
//! releases, generations and close marks. Each function is one transition;
//! the caller runs the ACP calls and actor registration it calls for.

use super::ExternalProviderRuntimeError;
use crate::SessionGeneration;
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent, SessionCellRefusal};
use crate::session_cell_table::SessionCellTable;
use crate::session_lifecycle_answers::refusal_answer;
use crate::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleRefusal,
};
use state_machine_core::TransitionError;

/// Admits a created identity: NotLoaded to Live under the run's next
/// generation, which it returns. A session already loading, live or closing
/// is a duplicate. `last_published` is the sink's generation for a session
/// this connection hasn't run, so a run here is newer than one on an earlier
/// connection.
pub(super) fn admit_create(
    cells: &SessionCellTable,
    session_id: &str,
    last_published: SessionGeneration,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(session_id, last_published, SessionCellEvent::CreateAdmitted)
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))?;
    effects
        .iter()
        .find_map(|effect| match effect {
            SessionCellEffect::Lifecycle(SessionLifecycleEffect::RegisterActor { generation }) => {
                Some(*generation)
            }
            SessionCellEffect::Lifecycle(_)
            | SessionCellEffect::Entry { .. }
            | SessionCellEffect::Turn(_) => None,
        })
        .ok_or(ExternalProviderRuntimeError::LocalBusy)
}

/// Returns an admitted run whose actor never registered to NotLoaded,
/// keeping its generation as the last one so the next run is newer.
pub(super) fn abandon_run(
    cells: &SessionCellTable,
    session_id: &str,
    generation: SessionGeneration,
) {
    // Stale only when the cell already moved on; nothing is left to undo.
    let _ignored = cells.transition(
        session_id,
        SessionGeneration::UNTRACKED,
        SessionCellEvent::ActorExited { generation },
    );
}

/// Admits a load or resume: NotLoaded to Loading under the run's next
/// generation, which it returns. Loading, live or closing sessions refuse
/// it. `last_published` seeds a session this connection hasn't run, as for
/// a create.
pub(super) fn admit_load(
    cells: &SessionCellTable,
    session_id: &str,
    last_published: SessionGeneration,
    via: RestoreVia,
    attempt: LoadAttempt,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(
            session_id,
            last_published,
            SessionCellEvent::LoadRequested { via, attempt },
        )
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))?;
    effects
        .iter()
        .find_map(|effect| match effect {
            SessionCellEffect::Lifecycle(SessionLifecycleEffect::StartRestore {
                generation,
                ..
            }) => Some(*generation),
            SessionCellEffect::Lifecycle(_)
            | SessionCellEffect::Entry { .. }
            | SessionCellEffect::Turn(_) => None,
        })
        .ok_or(ExternalProviderRuntimeError::LocalBusy)
}

/// The restore admitted as `attempt` is still the session's load: no close
/// or other load has replaced it since.
pub(super) fn is_current_load(
    cells: &SessionCellTable,
    session_id: &str,
    attempt: LoadAttempt,
) -> bool {
    cells.read(session_id, |cell| {
        matches!(
            cell.lifecycle,
            SessionLifecycle::Loading { attempt: current, .. } if current == attempt
        )
    })
}

/// Settles the load admitted as `attempt`: Live when its actor registered,
/// NotLoaded otherwise. A stale attempt changes nothing.
pub(super) fn settle_load(
    cells: &SessionCellTable,
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
    let _ignored = cells.transition(session_id, SessionGeneration::UNTRACKED, event);
}

/// Admits a close: Live or Loading to Closing. Closing refuses it as busy;
/// NotLoaded answers "nothing to close" with its next steps (M8).
pub(super) fn admit_close(
    cells: &SessionCellTable,
    session_id: &str,
    attempt: CloseAttempt,
) -> Result<(), ExternalProviderRuntimeError> {
    cells
        .transition(
            session_id,
            SessionGeneration::UNTRACKED,
            SessionCellEvent::CloseRequested { attempt },
        )
        .map(|_effects| ())
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))
}

/// Settles the close admitted as `attempt`. A done close ends NotLoaded and
/// returns the generation its Closed publishes under; a failed close goes
/// back to Live from Live and to NotLoaded from Loading.
pub(super) fn settle_close(
    cells: &SessionCellTable,
    session_id: &str,
    attempt: CloseAttempt,
    freed: bool,
) -> Option<SessionGeneration> {
    let event = if freed {
        SessionCellEvent::CloseDone { attempt }
    } else {
        SessionCellEvent::CloseFailed { attempt }
    };
    let effects = cells
        .transition(session_id, SessionGeneration::UNTRACKED, event)
        .ok()?;
    effects.iter().find_map(|effect| match effect {
        SessionCellEffect::Lifecycle(SessionLifecycleEffect::PublishClosed { generation }) => {
            Some(*generation)
        }
        SessionCellEffect::Lifecycle(_)
        | SessionCellEffect::Entry { .. }
        | SessionCellEffect::Turn(_) => None,
    })
}

/// The session is closing on this connection.
pub(super) fn is_closing(cells: &SessionCellTable, session_id: &str) -> bool {
    cells.read(session_id, |cell| {
        matches!(cell.lifecycle, SessionLifecycle::Closing { .. })
    })
}

/// The runtime error for a refused lifecycle move. A busy session keeps
/// today's LocalBusy; "nothing to close" answers with the state and the
/// valid next steps (M8).
fn lifecycle_refusal_error(
    cells: &SessionCellTable,
    session_id: &str,
    refusal: TransitionError<SessionCellRefusal>,
) -> ExternalProviderRuntimeError {
    let state = cells.read(session_id, |cell| cell.lifecycle);
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
