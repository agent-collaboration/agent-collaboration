//! The connection's typed session states (D1 state machines, design note
//! §11–§16): the lifecycle moves on each provider session's `SessionCell`,
//! replacing the connection's separate sets of pending loads, pending closes,
//! releases, generations and close marks. Each function is one transition;
//! the caller runs the ACP calls and actor registration it calls for.

use crate::provider_client_contract::{
    ExternalProviderRuntimeError, ProviderPromptDispatchObservation,
};
use crate::session_cell_table::SessionCellTable;
use provider_session_machines::SessionGeneration;
use provider_session_machines::queued_entry_machine::SentOutcome;
use provider_session_machines::session_cell_machine::{
    SessionCellEffect, SessionCellEvent, SessionCellRefusal,
};
use provider_session_machines::session_lifecycle_answers::{refusal_answer, state_answer};
use provider_session_machines::session_lifecycle_machine::{
    CloseAttempt, LoadAttempt, RestoreVia, SessionLifecycle, SessionLifecycleEffect,
    SessionLifecycleRefusal,
};
use provider_session_machines::session_refusal_answers::{
    session_busy_answer, settings_hold_answer,
};
use provider_session_machines::session_turn_machine::TurnId;
use state_machine_core::TransitionError;

/// Admits a created identity: NotLoaded to Live under the run's next
/// generation, which it returns. A session already loading, live or closing
/// is a duplicate.
pub(crate) fn admit_create(
    cells: &SessionCellTable,
    session_id: &str,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(
            session_id,
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
pub(crate) fn abandon_run(
    cells: &SessionCellTable,
    session_id: &str,
    generation: SessionGeneration,
) {
    // Stale only when the cell already moved on; nothing is left to undo.
    let _ignored = cells.transition(
        session_id,
        SessionCellEvent::ActorExited { generation },
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
}

/// Admits a load or resume: NotLoaded to Loading under the run's next
/// generation, which it returns. Loading, live or closing sessions refuse
/// it.
pub(crate) fn admit_load(
    cells: &SessionCellTable,
    session_id: &str,
    via: RestoreVia,
    attempt: LoadAttempt,
) -> Result<SessionGeneration, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(
            session_id,
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
pub(crate) fn is_current_load(
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
pub(crate) fn settle_load(
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
        event,
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
}

/// Admits a close: Live or Loading to Closing. Closing refuses it as busy;
/// NotLoaded answers "nothing to close" with its next steps (M8).
pub(crate) fn admit_close(
    cells: &SessionCellTable,
    session_id: &str,
    attempt: CloseAttempt,
) -> Result<(), ExternalProviderRuntimeError> {
    cells
        .transition(
            session_id,
            SessionCellEvent::CloseRequested { attempt },
            crate::session_cell_table::EntryEffectRoute::Sink,
        )
        .map(|_effects| ())
        .map_err(|refusal| lifecycle_refusal_error(cells, session_id, refusal))
}

/// Settles the close admitted as `attempt`. A done close ends NotLoaded and
/// returns the generation its Closed publishes under; a failed close goes
/// back to Live from Live and to NotLoaded from Loading.
pub(crate) fn settle_close(
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
pub(crate) struct DirectTurnRefusal {
    pub(crate) observation: ProviderPromptDispatchObservation,
    pub(crate) error: ExternalProviderRuntimeError,
}

/// Opens the session's turn for a direct prompt: only while Live, with no
/// turn running and no queued message in flight. A refusal answers with the
/// session's state and the steps the agent allows (M8).
pub(crate) fn start_direct_turn(
    cells: &SessionCellTable,
    session_id: &str,
    turn: TurnId,
    agent_steers: bool,
) -> Result<(), Box<DirectTurnRefusal>> {
    let refusal = match cells.transition(
        session_id,
        SessionCellEvent::PromptStarted { turn },
        crate::session_cell_table::EntryEffectRoute::Sink,
    ) {
        Ok(_effects) => return Ok(()),
        Err(refusal) => refusal,
    };
    let busy = || {
        let answer = busy_answer(cells, session_id, agent_steers);
        DirectTurnRefusal {
            observation: ProviderPromptDispatchObservation::Refused(
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
            observation: ProviderPromptDispatchObservation::Refused(
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
            | SessionCellRefusal::Setting(_)
            | SessionCellRefusal::RestoreModeWriteUnsettled,
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => busy(),
    }))
}

/// Ends a session's turn when dropped. The outcome is unknown unless the
/// prompt was shown applied or never sent (design note §16 refines this
/// once the operation record decides it).
pub(crate) struct CellTurnGuard {
    cells: std::sync::Arc<SessionCellTable>,
    session_id: String,
    turn: TurnId,
    outcome: SentOutcome,
}

impl CellTurnGuard {
    pub(crate) fn new(
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
    pub(crate) fn applied(&mut self) {
        self.outcome = SentOutcome::Applied;
    }

    /// The prompt was refused before it was sent.
    pub(crate) fn not_sent(&mut self) {
        self.outcome = SentOutcome::Refused;
    }
}

impl Drop for CellTurnGuard {
    fn drop(&mut self) {
        // Stale only when the turn already ended (a close freed it).
        let _ended = self.cells.transition(
            &self.session_id,
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
pub(crate) async fn wait_for_close_turn(cells: &SessionCellTable, session_id: &str) {
    #[cfg(feature = "test-support")]
    crate::sim_points::reach(crate::sim_points::ClientSimPoint::CloseTurnWait).await;
    cells
        .wait_while(session_id, |cell| {
            matches!(
                cell.lifecycle,
                SessionLifecycle::Closing {
                    phase: provider_session_machines::session_lifecycle_machine::ClosePhase::EndingTurn,
                    ..
                }
            )
        })
        .await;
}

/// An actor's loop ended on its own: Live{g} goes to NotLoaded, the line
/// ends (its waiting messages settle "the agent stopped" through the queue
/// sink), and the hub reads Unloaded for that run. Any other state means a
/// close or a newer run already moved past this actor: nothing changes, and
/// it answers false. The connection loop calls it (Lead V1), so the move and
/// its Unloaded happen before the loop admits anything else.
pub(crate) fn report_actor_exit(
    cells: &SessionCellTable,
    event_sink: &dyn crate::SessionEventSink,
    session_id: &str,
    generation: SessionGeneration,
) -> bool {
    let Ok(effects) = cells.transition(
        session_id,
        SessionCellEvent::ActorExited { generation },
        crate::session_cell_table::EntryEffectRoute::Sink,
    ) else {
        return false;
    };
    for effect in effects {
        if let SessionCellEffect::Lifecycle(SessionLifecycleEffect::PublishUnloaded {
            generation,
        }) = effect
        {
            let _published = event_sink.publish_unloaded(session_id, generation);
        }
    }
    true
}

/// A setting kind holds the session's prompts: its last change was refused
/// where refusals hold, or its outcome is unknown. An in-flight change holds
/// nothing here: a prompt waits behind it in the session's actor.
pub(crate) fn settings_held(cells: &SessionCellTable, session_id: &str) -> bool {
    cells.read(
        session_id,
        provider_session_machines::session_cell_machine::SessionCell::settings_held,
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
        provider_session_machines::session_cell_machine::SessionCell::any_setting_outcome_unknown,
    );
    settings_hold_answer(if outcome_unknown {
        provider_session_machines::session_setting_machine::UnresolvedCause::OutcomeUnknown
    } else {
        provider_session_machines::session_setting_machine::UnresolvedCause::DefiniteFailure
    })
}

/// The kind's last change has an unknown outcome.
pub(crate) fn setting_outcome_unknown(
    cells: &SessionCellTable,
    session_id: &str,
    kind: provider_session_machines::ProviderSettingKind,
) -> bool {
    cells.read(session_id, |cell| cell.setting_outcome_unknown(kind))
}

/// Applies one setting kind's event; a stale result changes nothing.
pub(crate) fn apply_setting(
    cells: &SessionCellTable,
    session_id: &str,
    kind: provider_session_machines::ProviderSettingKind,
    event: provider_session_machines::session_setting_machine::SessionSettingEvent,
) {
    let _applied = cells.transition(
        session_id,
        SessionCellEvent::Setting { kind, event },
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
}

/// A setting event from run `generation`'s own work, such as the Lost of a
/// change whose reply never came back: it applies only while that run is
/// the session's current one (Lead V4).
pub(crate) fn apply_setting_in_run(
    cells: &SessionCellTable,
    session_id: &str,
    generation: SessionGeneration,
    kind: provider_session_machines::ProviderSettingKind,
    event: provider_session_machines::session_setting_machine::SessionSettingEvent,
) {
    let _applied = cells.transition_in_run(
        session_id,
        generation,
        SessionCellEvent::Setting { kind, event },
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
}

/// Whether the caller wants the session's fast kept off (note §12.3, R2-3):
/// the session's hold, kept across actor exits and reloads until R9's
/// events end it.
pub(crate) fn keep_fast_off(cells: &SessionCellTable, session_id: &str) -> bool {
    cells.read(session_id, |cell| cell.keep_fast_off)
}

/// Sets or ends the fast hold from run `generation`'s work; an ended run's
/// late write changes nothing (note §12.3; Lead, keep-fast-off point).
pub(crate) fn set_keep_fast_off_in_run(
    cells: &SessionCellTable,
    session_id: &str,
    generation: SessionGeneration,
    keep: bool,
) {
    let _set = cells.transition_in_run(
        session_id,
        generation,
        SessionCellEvent::KeepFastOff(keep),
        crate::session_cell_table::EntryEffectRoute::Caller,
    );
}

/// A setting the session couldn't take when it was created holds it: a
/// refused value (any kind, as today) or an unknown outcome.
pub(crate) fn hold_setting_at_create(
    cells: &SessionCellTable,
    session_id: &str,
    kind: provider_session_machines::ProviderSettingKind,
    cause: provider_session_machines::session_setting_machine::UnresolvedCause,
) {
    use provider_session_machines::session_setting_machine::{
        SessionSettingEvent, SettingApplication, SettingValueId,
    };
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
            provider_session_machines::session_setting_machine::UnresolvedCause::DefiniteFailure => {
                SessionSettingEvent::Failed { application }
            }
            provider_session_machines::session_setting_machine::UnresolvedCause::OutcomeUnknown => {
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
) -> Result<Vec<provider_session_machines::ProviderSettingKind>, ExternalProviderRuntimeError> {
    let effects = cells
        .transition(
            session_id,
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
pub(crate) fn is_closing(cells: &SessionCellTable, session_id: &str) -> bool {
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
            | SessionCellRefusal::Setting(_)
            | SessionCellRefusal::RestoreModeWriteUnsettled,
        )
        | TransitionError::NotAllowed { .. }
        | TransitionError::Stale => ExternalProviderRuntimeError::LocalBusy {
            answer: state_answer(&state),
        },
    }
}

/// A restore's claim on writing the session's recorded mode to the agent
/// (review F1): granted only while its load attempt still owns the session,
/// so a close that came first means nothing is sent. Dropping the claim, once
/// the write is answered or abandoned, settles it, and a later load waits for
/// that.
pub(crate) struct RestoreModeWriteClaim {
    cells: std::sync::Arc<SessionCellTable>,
    session_id: String,
    attempt: LoadAttempt,
}

/// Claims the restore's mode write for `attempt`, or None when that load
/// attempt no longer owns the session.
pub(crate) fn claim_restore_mode_write(
    cells: &std::sync::Arc<SessionCellTable>,
    session_id: &str,
    attempt: LoadAttempt,
) -> Option<RestoreModeWriteClaim> {
    cells
        .transition(
            session_id,
            SessionCellEvent::RestoreModeWriteClaimed { attempt },
            crate::session_cell_table::EntryEffectRoute::Sink,
        )
        .ok()
        .map(|_effects| RestoreModeWriteClaim {
            cells: std::sync::Arc::clone(cells),
            session_id: session_id.to_owned(),
            attempt,
        })
}

impl Drop for RestoreModeWriteClaim {
    fn drop(&mut self) {
        let _settled = self.cells.transition(
            &self.session_id,
            SessionCellEvent::RestoreModeWriteSettled {
                attempt: self.attempt,
            },
            crate::session_cell_table::EntryEffectRoute::Sink,
        );
    }
}

/// Test support: ends `generation`'s run as an actor exit does and admits a
/// newer run of the session, answering its generation.
#[cfg(test)]
pub(crate) fn end_run_and_load_next_for_test(
    cells: &SessionCellTable,
    session_id: &str,
    generation: SessionGeneration,
) -> SessionGeneration {
    let ended = cells.transition(
        session_id,
        SessionCellEvent::ActorExited { generation },
        crate::session_cell_table::EntryEffectRoute::Sink,
    );
    assert!(ended.is_ok(), "the run ends: {ended:?}");
    let attempt = LoadAttempt(uuid::Uuid::now_v7());
    let next = admit_load(cells, session_id, RestoreVia::Load, attempt);
    settle_load(cells, session_id, attempt, true);
    next.unwrap_or_else(|error| panic!("a newer run loads: {error:?}"))
}

#[cfg(test)]
#[path = "session_cell_bookkeeping_test.rs"]
mod tests;
