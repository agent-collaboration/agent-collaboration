//! Translate a committed ACP owner batch at the collaboration hub boundary.

use acp_client_runtime::session_owner_task::{OwnerCompletionPort, OwnerTaskError};
use collaboration_runtime::owner_inbox::{
    HubHistoryAvailability, HubPublication, HubPublicationBaseline, HubPublicationFact,
    HubPublicationStamp, HubSessionInbox,
};
use provider_session_machines::{
    session_owner::{
        BatchId, HistoryAvailability, OwnerAddress, OwnerEvent, PublicationBatch, PublicationFact,
        PublicationResult, SessionPublication,
    },
    session_turn_machine::SessionTurn,
};
use router_clock::RuntimeClock;
use session_event_model::{SessionEvent, SessionState, TurnLostReason, TurnOutcome};
use std::sync::Arc;
use tokio_util::task::TaskTracker;

pub struct SessionOwnerPublisher {
    inbox: HubSessionInbox,
    clock: Arc<dyn RuntimeClock>,
    tasks: TaskTracker,
}

impl SessionOwnerPublisher {
    pub fn new(inbox: HubSessionInbox, clock: Arc<dyn RuntimeClock>, tasks: TaskTracker) -> Self {
        Self {
            inbox,
            clock,
            tasks,
        }
    }

    pub fn publish(
        &self,
        owner: OwnerAddress,
        batch: PublicationBatch,
        completions: &OwnerCompletionPort,
    ) -> Result<Option<OwnerEvent>, OwnerTaskError> {
        let (id, batch) = adapt_batch(batch, self.clock.utc_now().timestamp());
        let refused = || Some(OwnerEvent::Publication(PublicationResult::Refused(id)));
        let completion = match completions.reserve(owner) {
            Ok(completion) => completion,
            Err(OwnerTaskError::CompletionCapacity) => return Ok(refused()),
            Err(error) => return Err(error),
        };
        let receipt = match self.inbox.try_offer(batch) {
            Ok(receipt) => receipt,
            Err(_) => return Ok(refused()),
        };
        self.tasks.spawn(async move {
            let result = match receipt.await {
                Ok(Ok(applied)) if applied == stamp(id) => PublicationResult::Applied(id),
                _ => PublicationResult::Refused(id),
            };
            completion.send(OwnerEvent::Publication(result));
        });
        Ok(None)
    }
}

fn stamp(id: BatchId) -> HubPublicationStamp {
    HubPublicationStamp {
        incarnation: id.incarnation.0,
        sequence: id.sequence,
    }
}

fn adapt_batch(batch: PublicationBatch, observed_at_seconds: i64) -> (BatchId, HubPublication) {
    match batch {
        PublicationBatch::Commit { id, facts } => {
            let mut hub = Vec::with_capacity(facts.len());
            for fact in facts {
                adapt_fact(fact, &mut hub);
            }
            (
                id,
                HubPublication::Commit {
                    stamp: stamp(id),
                    facts: hub,
                    observed_at_seconds,
                },
            )
        }
        PublicationBatch::Rebase {
            id,
            baseline,
            history,
        } => {
            let history = match history {
                HistoryAvailability::Available => HubHistoryAvailability::Available,
                HistoryAvailability::Unavailable => HubHistoryAvailability::Unavailable,
            };
            let current = baseline.projection;
            (
                id,
                HubPublication::Rebase {
                    stamp: stamp(id),
                    baseline: Box::new(HubPublicationBaseline {
                        state: current.state,
                        turn_running: !matches!(baseline.turn, SessionTurn::Idle),
                        capabilities: current.capabilities,
                        settings: current.settings,
                        skills: current.skills,
                        history,
                    }),
                },
            )
        }
    }
}

fn adapt_fact(fact: PublicationFact, hub: &mut Vec<HubPublicationFact>) {
    let event = match fact {
        PublicationFact::ReplayBoundary(_) => {
            hub.push(HubPublicationFact::ReplayBoundary);
            return;
        }
        PublicationFact::HistoryUnavailable(_) => {
            hub.push(HubPublicationFact::HistoryUnavailable);
            return;
        }
        PublicationFact::Skills { view, .. } => {
            hub.push(HubPublicationFact::Skills(view));
            return;
        }
        PublicationFact::Replay { event, .. } => event,
        PublicationFact::Capabilities { report, .. } => SessionEvent::CapabilitiesChanged {
            capabilities: report,
        },
        PublicationFact::Settings { settings, .. } => SessionEvent::SettingsChanged { settings },
        PublicationFact::Stream { event, .. } => event,
        PublicationFact::Live(
            SessionPublication::Created(_) | SessionPublication::Activated { .. },
        ) => SessionEvent::StateChanged {
            state: SessionState::Idle,
        },
        PublicationFact::Live(SessionPublication::Closed(_)) => SessionEvent::StateChanged {
            state: SessionState::Closed,
        },
        PublicationFact::Live(SessionPublication::Unloaded(_)) => SessionEvent::StateChanged {
            state: SessionState::Unloaded,
        },
        PublicationFact::Live(SessionPublication::TurnStarted { turn, input, .. }) => {
            SessionEvent::TurnStarted {
                turn_id: turn.0.to_string(),
                input_id: input,
            }
        }
        PublicationFact::Live(SessionPublication::TurnEnded {
            turn,
            outcome,
            idle,
            ..
        }) => {
            if idle
                && matches!(
                    outcome,
                    TurnOutcome::Lost {
                        reason: TurnLostReason::ProviderRetired
                            | TurnLostReason::ProviderTurnFailed
                    }
                )
            {
                hub.push(HubPublicationFact::Event(SessionEvent::TurnEnded {
                    turn_id: turn.0.to_string(),
                    outcome,
                }));
                SessionEvent::StateChanged {
                    state: SessionState::Idle,
                }
            } else {
                SessionEvent::TurnEnded {
                    turn_id: turn.0.to_string(),
                    outcome,
                }
            }
        }
        PublicationFact::Live(SessionPublication::PreviousTurnEnded { turn, outcome, .. }) => {
            hub.push(HubPublicationFact::PreviousTurnEnded {
                turn_id: turn.0.to_string(),
                outcome,
            });
            return;
        }
        // The rich confirmed settings stream carries ACP's original option IDs.
        PublicationFact::Live(SessionPublication::SettingObserved { .. }) => return,
    };
    hub.push(HubPublicationFact::Event(event));
}
