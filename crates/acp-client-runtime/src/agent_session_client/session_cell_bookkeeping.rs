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
use crate::session_lifecycle_answers::{refusal_answer, state_answer};
use crate::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleRefusal,
};
use crate::session_refusal_answers::{session_busy_answer, settings_hold_answer};
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
            | SessionCellEffect::Turn(_)
            | SessionCellEffect::SettingAccepted { .. } => None,
        })
        .ok_or_else(|| lifecycle_busy(cells, session_id))
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
            | SessionCellEffect::Turn(_)
            | SessionCellEffect::SettingAccepted { .. } => None,
        })
        .ok_or_else(|| lifecycle_busy(cells, session_id))
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
        | SessionCellEffect::Turn(_)
        | SessionCellEffect::SettingAccepted { .. } => None,
    })
}

/// Why a direct prompt couldn't open the session's turn: what its dispatch
/// observer hears and what the caller gets, as before the cell decided it.
pub(super) struct DirectTurnRefusal {
    pub(super) observation: super::ProviderPromptDispatchObservation,
    pub(super) error: ExternalProviderRuntimeError,
}

/// Opens the session's turn for a direct prompt: only while Live, with no
/// turn running and no queued message in flight. A refusal answers with the
/// session's state and the steps the agent allows (M8).
pub(super) fn start_direct_turn(
    cells: &SessionCellTable,
    session_id: &str,
    turn: TurnId,
    agent_steers: bool,
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
    let busy = || {
        let answer = busy_answer(cells, session_id, agent_steers);
        DirectTurnRefusal {
            observation: super::ProviderPromptDispatchObservation::Refused(
                crate::ProviderDispatchRefusal::Busy {
                    answer: answer.clone(),
                },
            ),
            error: ExternalProviderRuntimeError::LocalBusy { answer },
        }
    };
    Err(Box::new(match refusal {
        TransitionError::Domain(SessionCellRefusal::Lifecycle(
            refusal @ SessionLifecycleRefusal::NotLoaded,
        )) => DirectTurnRefusal {
            observation: super::ProviderPromptDispatchObservation::Refused(
                crate::ProviderDispatchRefusal::NotLoaded {
                    answer: refusal_answer(&cells.read(session_id, |cell| cell.lifecycle), refusal),
                },
            ),
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
            | SessionCellRefusal::NotLive
            | SessionCellRefusal::Setting(_),
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => busy(),
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

/// Named points a scenario test can observe or hold (design note §11.8),
/// through Turmoil's barriers. Compiled only for this crate's tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ClientSimPoint {
    /// A close reached its wait for the session's turn end.
    CloseTurnWait,
    /// A prompt's turn is open on the session's cell; its command hasn't
    /// reached the connection yet.
    PromptTurnOpen,
}

/// Waits while the session's close is ending its running turn: the close
/// frees the session only after that turn's end is recorded.
pub(super) async fn wait_for_close_turn(cells: &SessionCellTable, session_id: &str) {
    #[cfg(test)]
    turmoil::barriers::trigger(ClientSimPoint::CloseTurnWait).await;
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

/// An actor's loop ended on its own: Live{g} goes to NotLoaded, the line
/// ends (its waiting messages settle "the agent stopped" through the queue
/// sink), and the hub reads Unloaded for that run. Any other state means a
/// close or a newer run already moved past this actor: nothing changes.
pub(crate) fn report_actor_exit(
    cells: &SessionCellTable,
    event_sink: &dyn crate::SessionEventSink,
    session_id: &str,
    generation: SessionGeneration,
) {
    let Ok(effects) = cells.transition(
        session_id,
        SessionGeneration::UNTRACKED,
        SessionCellEvent::ActorExited { generation },
        crate::session_cell_table::EntryEffectRoute::Sink,
    ) else {
        return;
    };
    for effect in effects {
        if let SessionCellEffect::Lifecycle(SessionLifecycleEffect::PublishUnloaded {
            generation,
        }) = effect
        {
            let _published = event_sink.publish_in(
                session_id,
                generation,
                session_event_model::SessionEvent::StateChanged {
                    state: session_event_model::SessionState::Unloaded,
                },
            );
        }
    }
}

/// A setting kind holds the session's prompts: its last change was refused
/// where refusals hold, or its outcome is unknown. An in-flight change holds
/// nothing here: a prompt waits behind it in the session's actor.
pub(crate) fn settings_held(cells: &SessionCellTable, session_id: &str) -> bool {
    cells.read(
        session_id,
        crate::session_cell_machine::SessionCell::settings_held,
    )
}

/// A busy refusal's answer, read from the session's cell: its lifecycle
/// state, or its turn and the steps the agent allows (D1 M8).
pub(crate) fn busy_answer(
    cells: &SessionCellTable,
    session_id: &str,
    agent_steers: bool,
) -> String {
    cells.read(session_id, |cell| {
        session_busy_answer(&cell.lifecycle, cell.turn, agent_steers)
    })
}

/// A held setting's answer, read from the session's cell (D1 M8).
pub(crate) fn settings_hold_answer_for(cells: &SessionCellTable, session_id: &str) -> String {
    let outcome_unknown = cells.read(
        session_id,
        crate::session_cell_machine::SessionCell::any_setting_outcome_unknown,
    );
    settings_hold_answer(if outcome_unknown {
        crate::session_setting_machine::UnresolvedCause::OutcomeUnknown
    } else {
        crate::session_setting_machine::UnresolvedCause::DefiniteFailure
    })
}

/// The kind's last change has an unknown outcome.
pub(crate) fn setting_outcome_unknown(
    cells: &SessionCellTable,
    session_id: &str,
    kind: crate::ProviderSettingKind,
) -> bool {
    cells.read(session_id, |cell| cell.setting_outcome_unknown(kind))
}

/// Applies one setting kind's event; a stale result changes nothing.
pub(crate) fn apply_setting(
    cells: &SessionCellTable,
    session_id: &str,
    kind: crate::ProviderSettingKind,
    event: crate::session_setting_machine::SessionSettingEvent,
) {
    let _applied = cells.transition(
        session_id,
        SessionGeneration::UNTRACKED,
        SessionCellEvent::Setting { kind, event },
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
}

/// A setting the session couldn't take when it was created holds it: a
/// refused value (any kind, as today) or an unknown outcome.
pub(crate) fn hold_setting_at_create(
    cells: &SessionCellTable,
    session_id: &str,
    kind: crate::ProviderSettingKind,
    cause: crate::session_setting_machine::UnresolvedCause,
) {
    use crate::session_setting_machine::{SessionSettingEvent, SettingApplication, SettingValueId};
    let application = SettingApplication(uuid::Uuid::now_v7());
    apply_setting(
        cells,
        session_id,
        kind,
        SessionSettingEvent::SetRequested {
            to: SettingValueId(String::new()),
            application,
        },
    );
    apply_setting(
        cells,
        session_id,
        kind,
        match cause {
            crate::session_setting_machine::UnresolvedCause::DefiniteFailure => {
                SessionSettingEvent::Failed { application }
            }
            crate::session_setting_machine::UnresolvedCause::OutcomeUnknown => {
                SessionSettingEvent::OutcomeUnknown { application }
            }
        },
    );
}

/// Accepts every refused setting at once (one session-cell event); an
/// in-flight change is left alone. Refused when any kind's outcome is
/// unknown. Returns the kinds it accepted.
pub(crate) fn accept_settings(
    cells: &SessionCellTable,
    session_id: &str,
) -> Result<Vec<crate::ProviderSettingKind>, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(
            session_id,
            SessionGeneration::UNTRACKED,
            SessionCellEvent::AcceptSettings,
            crate::session_cell_table::EntryEffectRoute::Sink,
        )
        .map_err(|_| ExternalProviderRuntimeError::SettingsOutcomeUncertain)?;
    Ok(effects
        .into_iter()
        .filter_map(|effect| match effect {
            SessionCellEffect::SettingAccepted { kind } => Some(kind),
            SessionCellEffect::Lifecycle(_)
            | SessionCellEffect::Queued { .. }
            | SessionCellEffect::Entry { .. }
            | SessionCellEffect::Turn(_) => None,
        })
        .collect())
}

/// The session is closing on this connection.
pub(super) fn is_closing(cells: &SessionCellTable, session_id: &str) -> bool {
    cells.read(session_id, |cell| {
        matches!(cell.lifecycle, SessionLifecycle::Closing { .. })
    })
}

/// A lifecycle move the session's state didn't answer with an effect: busy,
/// answered with the session's state and its valid next steps (M8).
fn lifecycle_busy(cells: &SessionCellTable, session_id: &str) -> ExternalProviderRuntimeError {
    ExternalProviderRuntimeError::LocalBusy {
        answer: state_answer(&cells.read(session_id, |cell| cell.lifecycle)),
    }
}

/// The runtime error for a refused lifecycle move. A busy session keeps
/// today's LocalBusy kind; every refusal answers with the state and the
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
        TransitionError::Domain(SessionCellRefusal::Lifecycle(
            refusal @ (SessionLifecycleRefusal::SessionLoading
            | SessionLifecycleRefusal::AlreadyLoaded
            | SessionLifecycleRefusal::SessionClosing
            | SessionLifecycleRefusal::NotLoaded
            | SessionLifecycleRefusal::CloseInProgress),
        )) => ExternalProviderRuntimeError::LocalBusy {
            answer: refusal_answer(&state, refusal),
        },
        TransitionError::Domain(
            SessionCellRefusal::Line(_)
            | SessionCellRefusal::Entry(_)
            | SessionCellRefusal::Turn(_)
            | SessionCellRefusal::NotHead
            | SessionCellRefusal::UnknownEntry
            | SessionCellRefusal::NotLive
            | SessionCellRefusal::Setting(_),
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => ExternalProviderRuntimeError::LocalBusy {
            answer: state_answer(&state),
        },
    }
}

#[cfg(test)]
#[path = "session_cell_bookkeeping_test.rs"]
mod tests;
