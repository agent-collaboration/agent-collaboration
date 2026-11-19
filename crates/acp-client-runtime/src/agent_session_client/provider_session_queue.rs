//! The Host's door to a session's queue line (D1 state machines, design note
//! §11.2, §12.2, §13.1–§13.2). Every call is one transition on the session's
//! cell, so admission, commit, the send's handover, cancel, close and stop
//! decide against the same state and exactly one wins. Each call returns what
//! the Host must do next; settlements of messages another move ended go to
//! the Host's `QueueEntrySink`.

use super::{AgentSessionClient, Arc, InteractionPort};
use crate::session_cell_table::{EntryEffectRoute, QueueEntrySink, SessionCellTable};
use provider_session_machines::SessionGeneration;
use provider_session_machines::queued_entry_machine::{QueuedEntryEffect, QueuedEntryRefusal};
use provider_session_machines::session_cell_machine::{
    QueuedOperation, SessionCellEffect, SessionCellEvent, SessionCellRefusal,
};
use provider_session_machines::session_lifecycle_answers::refusal_answer;
use provider_session_machines::session_lifecycle_machine::SessionLifecycleRefusal;
use provider_session_machines::session_turn_machine::TurnId;
use state_machine_core::TransitionError;

/// A message's place in its session's line: the run it joined and its
/// number, unique for the session on this connection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueTicket {
    pub generation: SessionGeneration,
    pub sequence: u64,
}

/// Why the line refused a move.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QueueRefusal {
    /// The session isn't loaded: a message that may load it loads it first;
    /// the answer says what the caller can do.
    NotLoaded { answer: String },
    /// The message isn't the line's next, the run isn't Live yet, or a sent
    /// message's turn is still running: wait and try again.
    NotYet,
    /// The message already ended: a close, a failed load, a stop or a cancel
    /// won, and its settlement went to the queue sink.
    EntryEnded,
    /// Refused in the session's current state, with the answer.
    Refused { answer: String },
}

/// One sent message's turn, minted at the handover; its end settles the
/// message and frees the line for the next one.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueueTurn(pub(crate) TurnId);

/// What follows a written dispatch mark.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueMarkStep {
    /// The handover: send the prompt under this turn.
    Send { turn: QueueTurn },
    /// A close, cancel or stop won while the mark was written: undo the mark,
    /// then report it with `queue_unmark_done`.
    Withdraw,
    /// The message already ended.
    EntryEnded,
}

/// What a cancel did.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueCancel {
    /// The message ended not sent with this reason; the caller settles it.
    NotSent { reason: &'static str },
    /// Its dispatch mark is being written: the runner that owns it settles
    /// it not sent once the write returns.
    Withdrawing,
}

/// The queue moves over one connection's session table; the client's
/// public methods delegate here.
pub(crate) struct SessionQueueDoor<'a> {
    pub(crate) cells: &'a SessionCellTable,
}

impl SessionQueueDoor<'_> {
    /// Admits `operation_id` into the session's line while it is Loading or
    /// Live.
    pub fn queue_enqueue(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueTicket, QueueRefusal> {
        let effects = self.queue_move(
            provider_session_id,
            SessionCellEvent::Enqueue {
                operation: QueuedOperation(operation_id.to_owned()),
            },
        )?;
        effects
            .iter()
            .find_map(|effect| match effect {
                SessionCellEffect::Queued {
                    generation,
                    sequence,
                } => Some(QueueTicket {
                    generation: *generation,
                    sequence: *sequence,
                }),
                SessionCellEffect::Lifecycle(_)
                | SessionCellEffect::Entry { .. }
                | SessionCellEffect::Turn(_)
                | SessionCellEffect::SettingAccepted { .. } => None,
            })
            .ok_or(QueueRefusal::EntryEnded)
    }

    /// The worker holds the line's next message.
    pub fn queue_claim(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.queue_move(provider_session_id, SessionCellEvent::Claim { sequence })
            .map(|_effects| ())
    }

    /// Commits the held head to a send and claims it for the caller, which
    /// then writes the dispatch mark (design note §12.2 steps 1–2). Refused
    /// unless the run is Live, the message is the head and nothing is in
    /// flight.
    pub fn queue_commit(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.queue_move(provider_session_id, SessionCellEvent::Commit { sequence })?;
        self.queue_move(
            provider_session_id,
            SessionCellEvent::StartSendClaimed { sequence },
        )
        .map(|_effects| ())
    }

    /// The dispatch mark was written; decides between the handover and a
    /// withdrawal under the session's lock (design note §13.1).
    pub fn queue_mark_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
        transport_open: bool,
    ) -> QueueMarkStep {
        let turn = TurnId(uuid::Uuid::now_v7());
        match self.queue_move(
            provider_session_id,
            SessionCellEvent::MarkCommitted {
                sequence,
                transport_open,
                turn,
            },
        ) {
            Ok(effects)
                if has_entry_effect(&effects, |effect| {
                    matches!(effect, QueuedEntryEffect::SendRequest)
                }) =>
            {
                QueueMarkStep::Send {
                    turn: QueueTurn(turn),
                }
            }
            Ok(_) => QueueMarkStep::Withdraw,
            Err(_) => QueueMarkStep::EntryEnded,
        }
    }

    /// The dispatch mark wasn't written (`uncertain`: its outcome is
    /// unknown). The message ends not sent; returns the reason the caller
    /// settles it with.
    pub fn queue_mark_not_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
        uncertain: bool,
    ) -> Option<&'static str> {
        let event = if uncertain {
            SessionCellEvent::MarkUncertain { sequence }
        } else {
            SessionCellEvent::MarkFailed { sequence }
        };
        self.queue_move(provider_session_id, event)
            .ok()
            .and_then(|effects| not_sent_reason(&effects))
    }

    /// A withdrawn message's mark was undone (`unmarked`) or couldn't be.
    /// Returns the reason the caller settles it not sent with.
    pub fn queue_unmark_done(
        &self,
        provider_session_id: &str,
        sequence: u64,
        unmarked: bool,
    ) -> Option<&'static str> {
        let event = if unmarked {
            SessionCellEvent::UnmarkCommitted { sequence }
        } else {
            SessionCellEvent::UnmarkFailed { sequence }
        };
        self.queue_move(provider_session_id, event)
            .ok()
            .and_then(|effects| not_sent_reason(&effects))
    }

    /// Cancels a waiting message. A sent one is refused: cancel its turn.
    pub fn queue_cancel(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueCancel, QueueRefusal> {
        let effects = self.queue_move(
            provider_session_id,
            SessionCellEvent::CancelEntry {
                operation: QueuedOperation(operation_id.to_owned()),
            },
        )?;
        Ok(match not_sent_reason(&effects) {
            Some(reason) => QueueCancel::NotSent { reason },
            None => QueueCancel::Withdrawing,
        })
    }

    /// Router shutdown: every line ends; messages not yet sent end not sent
    /// through the queue sink, and sent ones finish their turns.
    pub fn queue_stop(&self) {
        for session_id in self.cells.session_ids() {
            let _ended =
                self.cells
                    .transition(&session_id, SessionCellEvent::Stop, EntryEffectRoute::Sink);
        }
    }

    fn queue_move(
        &self,
        provider_session_id: &str,
        event: SessionCellEvent,
    ) -> Result<Vec<SessionCellEffect>, QueueRefusal> {
        self.cells
            .transition(provider_session_id, event, EntryEffectRoute::Caller)
            .map_err(|refusal| self.queue_refusal(provider_session_id, refusal))
    }

    fn queue_refusal(
        &self,
        provider_session_id: &str,
        refusal: TransitionError<SessionCellRefusal>,
    ) -> QueueRefusal {
        let state = self.cells.read(provider_session_id, |cell| cell.lifecycle);
        match refusal {
            TransitionError::Domain(SessionCellRefusal::Lifecycle(
                refusal @ SessionLifecycleRefusal::NotLoaded,
            )) => QueueRefusal::NotLoaded {
                answer: refusal_answer(&state, refusal),
            },
            TransitionError::Domain(SessionCellRefusal::Lifecycle(
                refusal @ (SessionLifecycleRefusal::DuplicateIdentity
                | SessionLifecycleRefusal::SessionLoading
                | SessionLifecycleRefusal::AlreadyLoaded
                | SessionLifecycleRefusal::SessionClosing
                | SessionLifecycleRefusal::NothingToClose
                | SessionLifecycleRefusal::CloseInProgress),
            )) => QueueRefusal::Refused {
                answer: refusal_answer(&state, refusal),
            },
            TransitionError::Domain(SessionCellRefusal::Line(refusal)) => QueueRefusal::Refused {
                answer: refusal.human_phrase(),
            },
            TransitionError::Domain(SessionCellRefusal::Entry(
                refusal @ QueuedEntryRefusal::AlreadySent,
            )) => QueueRefusal::Refused {
                answer: refusal.human_phrase().to_owned(),
            },
            TransitionError::Domain(SessionCellRefusal::Setting(refusal)) => {
                QueueRefusal::Refused {
                    answer: refusal.human_phrase().to_owned(),
                }
            }
            TransitionError::Domain(SessionCellRefusal::NotHead | SessionCellRefusal::NotLive) => {
                QueueRefusal::NotYet
            }
            TransitionError::Domain(
                SessionCellRefusal::UnknownEntry | SessionCellRefusal::Entry(_),
            )
            | TransitionError::NotAllowed { .. }
            | TransitionError::Stale => QueueRefusal::EntryEnded,
            TransitionError::Domain(SessionCellRefusal::Turn(refusal)) => QueueRefusal::Refused {
                answer: refusal.human_phrase().to_owned(),
            },
        }
    }
}

/// A load the session's cell admitted (Loading): messages may join its line
/// now; `finished` answers when the agent has loaded it.
pub struct SessionLoadStarted {
    pub generation: SessionGeneration,
    completion: tokio::sync::oneshot::Receiver<
        Result<super::RestoredSessionAdmission, super::ExternalProviderRuntimeError>,
    >,
}

impl SessionLoadStarted {
    /// The load's result: the run is Live, or it failed and the messages
    /// that waited for it ended not sent.
    pub async fn finished(
        self,
    ) -> Result<super::RestoredSessionAdmission, super::ExternalProviderRuntimeError> {
        self.completion
            .await
            .map_err(|_| super::ExternalProviderRuntimeError::RestoreDisconnected)?
    }
}

impl<P: InteractionPort> AgentSessionClient<P> {
    /// Starts loading the session (session/load with replay), rebuilding the
    /// setup its create had (D1 R18), and returns as soon as its cell admits
    /// the load, so a door can put messages in the loading run's line in
    /// order.
    pub async fn begin_load_session(
        &self,
        provider_session_id: String,
        cwd: std::path::PathBuf,
        setup: crate::SessionRestoreSetup,
    ) -> Result<SessionLoadStarted, super::ExternalProviderRuntimeError> {
        if !self.base_capabilities.supports_load {
            return Err(super::ExternalProviderRuntimeError::UnsupportedCapability {
                capability: "session/load",
            });
        }
        let (admitted, generation) = tokio::sync::oneshot::channel();
        let (reply, completion) = tokio::sync::oneshot::channel();
        self.commands
            .send(super::ProviderCommand::Restore {
                provider_session_id,
                cwd,
                mode: super::provider_session_restore::RestoreHistoryMode::Replay,
                setup,
                admitted: Some(admitted),
                reply,
            })
            .await
            .map_err(|_| super::ExternalProviderRuntimeError::TransportFailure)?;
        match generation.await {
            Ok(generation) => Ok(SessionLoadStarted {
                generation,
                completion,
            }),
            // Refused before admission: the reply carries why.
            Err(_) => Err(completion
                .await
                .map_err(|_| super::ExternalProviderRuntimeError::TransportFailure)?
                .err()
                .unwrap_or(super::ExternalProviderRuntimeError::TransportFailure)),
        }
    }

    /// Sets the Host's sink for messages that moves it didn't drive end.
    pub fn set_queue_entry_sink(&self, sink: Arc<dyn QueueEntrySink>) {
        self.session_cells.set_sink(sink);
    }

    fn queue_door(&self) -> SessionQueueDoor<'_> {
        SessionQueueDoor {
            cells: &self.session_cells,
        }
    }

    /// Admits `operation_id` into the session's line while it is Loading or
    /// Live.
    pub fn queue_enqueue(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueTicket, QueueRefusal> {
        self.queue_door()
            .queue_enqueue(provider_session_id, operation_id)
    }

    /// The worker holds the line's next message.
    pub fn queue_claim(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.queue_door().queue_claim(provider_session_id, sequence)
    }

    /// Commits the held head to a send and claims it for the caller.
    pub fn queue_commit(
        &self,
        provider_session_id: &str,
        sequence: u64,
    ) -> Result<(), QueueRefusal> {
        self.queue_door()
            .queue_commit(provider_session_id, sequence)
    }

    /// The dispatch mark was written: the handover or a withdrawal.
    pub fn queue_mark_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
        transport_open: bool,
    ) -> QueueMarkStep {
        self.queue_door()
            .queue_mark_written(provider_session_id, sequence, transport_open)
    }

    /// The dispatch mark wasn't written; returns the not-sent reason.
    pub fn queue_mark_not_written(
        &self,
        provider_session_id: &str,
        sequence: u64,
        uncertain: bool,
    ) -> Option<&'static str> {
        self.queue_door()
            .queue_mark_not_written(provider_session_id, sequence, uncertain)
    }

    /// A withdrawn message's mark was undone or couldn't be.
    pub fn queue_unmark_done(
        &self,
        provider_session_id: &str,
        sequence: u64,
        unmarked: bool,
    ) -> Option<&'static str> {
        self.queue_door()
            .queue_unmark_done(provider_session_id, sequence, unmarked)
    }

    /// Cancels a waiting message.
    pub fn queue_cancel(
        &self,
        provider_session_id: &str,
        operation_id: &str,
    ) -> Result<QueueCancel, QueueRefusal> {
        self.queue_door()
            .queue_cancel(provider_session_id, operation_id)
    }

    /// Router shutdown: every line ends.
    pub fn queue_stop(&self) {
        self.queue_door().queue_stop();
    }
}

fn has_entry_effect(
    effects: &[SessionCellEffect],
    wanted: impl Fn(&QueuedEntryEffect) -> bool,
) -> bool {
    effects.iter().any(|effect| match effect {
        SessionCellEffect::Entry { effect, .. } => wanted(effect),
        SessionCellEffect::Lifecycle(_)
        | SessionCellEffect::Queued { .. }
        | SessionCellEffect::Turn(_)
        | SessionCellEffect::SettingAccepted { .. } => false,
    })
}

/// The reason a move ended its message not sent, when it did.
fn not_sent_reason(effects: &[SessionCellEffect]) -> Option<&'static str> {
    effects.iter().find_map(|effect| match effect {
        SessionCellEffect::Entry {
            effect:
                QueuedEntryEffect::SettleNotSent(reason)
                | QueuedEntryEffect::ReadBackRow(reason)
                | QueuedEntryEffect::OverlayNotSentOverDispatch(reason),
            ..
        } => Some(reason.human_phrase()),
        SessionCellEffect::Entry {
            effect:
                QueuedEntryEffect::StartSend
                | QueuedEntryEffect::WriteMark
                | QueuedEntryEffect::SendRequest
                | QueuedEntryEffect::Unmark
                | QueuedEntryEffect::SettleSent(_),
            ..
        }
        | SessionCellEffect::Lifecycle(_)
        | SessionCellEffect::Queued { .. }
        | SessionCellEffect::Turn(_)
        | SessionCellEffect::SettingAccepted { .. } => None,
    })
}

#[cfg(test)]
#[path = "provider_session_queue_test.rs"]
mod tests;
