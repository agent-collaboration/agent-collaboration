//! The connection's typed session states (D1 state machines, design note
//! §11–§16): the lifecycle moves on each provider session's `SessionCell`,
//! replacing the connection's separate sets of pending loads, pending closes,
//! releases, generations and close marks. Each function is one transition;
//! the caller runs the ACP calls and actor registration it calls for.

use super::ExternalProviderRuntimeError;
use crate::SessionGeneration;
use crate::queued_entry_machine::SentOutcome;
use crate::session_cell_machine::{SessionCellEffect, SessionCellEvent, SessionCellRefusal};
use crate::session_cell_table::SessionCellTable;
use crate::session_lifecycle_answers::refusal_answer;
use crate::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleRefusal,
};
use crate::session_turn_machine::TurnId;
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
        .transition(
            session_id,
            last_published,
            SessionCellEvent::CreateAdmitted,
            crate::session_cell_table::EntryEffectRoute::Sink,
        )
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))?;
    effects
        .iter()
        .find_map(|effect| match effect {
            SessionCellEffect::Lifecycle(SessionLifecycleEffect::RegisterActor { generation }) => {
                Some(*generation)
            }
            SessionCellEffect::Lifecycle(_)
            | SessionCellEffect::Queued { .. }
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
        crate::session_cell_table::EntryEffectRoute::Sink,
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
            crate::session_cell_table::EntryEffectRoute::Sink,
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
            | SessionCellEffect::Queued { .. }
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
    let _ignored = cells.transition(
        session_id,
        SessionGeneration::UNTRACKED,
        event,
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
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
            crate::session_cell_table::EntryEffectRoute::Sink,
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
        .transition(
            session_id,
            SessionGeneration::UNTRACKED,
            event,
            crate::session_cell_table::EntryEffectRoute::Sink,
        )
        .ok()?;
    effects.iter().find_map(|effect| match effect {
        SessionCellEffect::Lifecycle(SessionLifecycleEffect::PublishClosed { generation }) => {
            Some(*generation)
        }
        SessionCellEffect::Lifecycle(_)
        | SessionCellEffect::Queued { .. }
        | SessionCellEffect::Entry { .. }
        | SessionCellEffect::Turn(_) => None,
    })
}

/// Why a direct prompt couldn't open the session's turn: what its dispatch
/// observer hears and what the caller gets, as before the cell decided it.
pub(super) struct DirectTurnRefusal {
    pub(super) observation: super::ProviderPromptDispatchObservation,
    pub(super) error: ExternalProviderRuntimeError,
}

/// Opens the session's turn for a direct prompt: only while Live, with no
/// turn running and no queued message in flight.
pub(super) fn start_direct_turn(
    cells: &SessionCellTable,
    session_id: &str,
    turn: TurnId,
) -> Result<(), Box<DirectTurnRefusal>> {
    let refusal = match cells.transition(
        session_id,
        SessionGeneration::UNTRACKED,
        SessionCellEvent::PromptStarted { turn },
        crate::session_cell_table::EntryEffectRoute::Sink,
    ) {
        Ok(_effects) => return Ok(()),
        Err(refusal) => refusal,
    };
    let busy = DirectTurnRefusal {
        observation: super::ProviderPromptDispatchObservation::Refused(
            crate::ProviderDispatchRefusal::Busy,
        ),
        error: ExternalProviderRuntimeError::LocalBusy,
    };
    Err(Box::new(match refusal {
        TransitionError::Domain(SessionCellRefusal::Lifecycle(
            SessionLifecycleRefusal::NotLoaded,
        )) => DirectTurnRefusal {
            observation: super::ProviderPromptDispatchObservation::NotSubmitted,
            error: ExternalProviderRuntimeError::LocalNotFound,
        },
        TransitionError::Domain(
            SessionCellRefusal::Lifecycle(
                SessionLifecycleRefusal::SessionClosing
                | SessionLifecycleRefusal::DuplicateIdentity
                | SessionLifecycleRefusal::SessionLoading
                | SessionLifecycleRefusal::AlreadyLoaded
                | SessionLifecycleRefusal::NothingToClose
                | SessionLifecycleRefusal::CloseInProgress,
            )
            | SessionCellRefusal::Turn(_)
            | SessionCellRefusal::Line(_)
            | SessionCellRefusal::Entry(_)
            | SessionCellRefusal::NotHead
            | SessionCellRefusal::UnknownEntry
            | SessionCellRefusal::NotLive,
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => busy,
    }))
}

/// Ends a session's turn when dropped. The outcome is unknown unless the
/// prompt was shown applied or never sent (design note §16 refines this
/// once the operation record decides it).
pub(super) struct CellTurnGuard {
    cells: std::sync::Arc<SessionCellTable>,
    session_id: String,
    turn: TurnId,
    outcome: SentOutcome,
}

impl CellTurnGuard {
    pub(super) fn new(
        cells: &std::sync::Arc<SessionCellTable>,
        session_id: &str,
        turn: TurnId,
    ) -> Self {
        Self {
            cells: std::sync::Arc::clone(cells),
            session_id: session_id.to_owned(),
            turn,
            outcome: SentOutcome::Unknown,
        }
    }

    /// The prompt returned its turn's result.
    pub(super) fn applied(&mut self) {
        self.outcome = SentOutcome::Applied;
    }

    /// The prompt was refused before it was sent.
    pub(super) fn not_sent(&mut self) {
        self.outcome = SentOutcome::Refused;
    }
}

impl Drop for CellTurnGuard {
    fn drop(&mut self) {
        // Stale only when the turn already ended (a close freed it).
        let _ended = self.cells.transition(
            &self.session_id,
            SessionGeneration::UNTRACKED,
            SessionCellEvent::TurnEnded {
                turn: self.turn,
                outcome: self.outcome,
            },
            crate::session_cell_table::EntryEffectRoute::Sink,
        );
    }
}

/// Waits while the session's close is ending its running turn: the close
/// frees the session only after that turn's end is recorded.
pub(super) async fn wait_for_close_turn(cells: &SessionCellTable, session_id: &str) {
    cells
        .wait_while(session_id, |cell| {
            matches!(
                cell.lifecycle,
                SessionLifecycle::Closing {
                    phase: crate::session_lifecycle_machine::ClosePhase::EndingTurn,
                    ..
                }
            )
        })
        .await;
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
