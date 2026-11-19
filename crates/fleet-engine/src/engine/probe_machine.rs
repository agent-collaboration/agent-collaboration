#![deny(clippy::wildcard_enum_match_arm)]
//! The test machine (P9): a pure reducer with free due times, behind test-support.
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError,
};
use serde::{Deserialize, Serialize};

pub struct EngineProbe;
#[cfg(test)]
#[path = "probe_machine_test.rs"]
mod tests;
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProbeState {
    Waiting {
        group: uuid::Uuid,
        due: Option<EngineTime>,
        claim: String,
    },
    Fired,
    Stopped,
}
impl StateTag for ProbeState {
    fn state_tag(&self) -> &'static str {
        match self {
            Self::Waiting { .. } => "waiting",
            Self::Fired => "fired",
            Self::Stopped => "stopped",
        }
    }
    fn is_terminal(&self) -> bool {
        match self {
            Self::Waiting { .. } => false,
            Self::Fired | Self::Stopped => true,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProbeEvent {
    Due { occurrence: EngineTime },
    Run,
    Stop,
}
impl EventTag for ProbeEvent {
    fn event_tag(&self) -> &'static str {
        match self {
            Self::Due { .. } => "due",
            Self::Run => "run",
            Self::Stop => "stop",
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProbeEffect {
    BumpGroup { group: uuid::Uuid, at: EngineTime },
    Claim,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ProbeRefusal {
    AlreadyFinished,
}
impl RefusalTag for ProbeRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadyFinished => "already_finished",
        }
    }
}
impl Machine for EngineProbe {
    const KIND: MachineKind = MachineKind("engine_probe");
    type State = ProbeState;
    type Event = ProbeEvent;
    type Effect = ProbeEffect;
    type Error = ProbeRefusal;
    fn transition(
        state: &ProbeState,
        event: ProbeEvent,
        context: &mut Context<'_>,
    ) -> Result<(ProbeState, Vec<ProbeEffect>), TransitionError<ProbeRefusal>> {
        match (state, event) {
            (ProbeState::Waiting { group, due, .. }, ProbeEvent::Due { occurrence }) => {
                if *due != Some(occurrence) {
                    return Err(TransitionError::Stale);
                }
                Ok((
                    ProbeState::Fired,
                    vec![
                        ProbeEffect::BumpGroup {
                            group: *group,
                            at: context.now,
                        },
                        ProbeEffect::Claim,
                    ],
                ))
            }
            (ProbeState::Waiting { group, .. }, ProbeEvent::Run) => Ok((
                ProbeState::Fired,
                vec![
                    ProbeEffect::BumpGroup {
                        group: *group,
                        at: context.now,
                    },
                    ProbeEffect::Claim,
                ],
            )),
            (ProbeState::Waiting { .. }, ProbeEvent::Stop) => Ok((ProbeState::Stopped, vec![])),
            (ProbeState::Fired | ProbeState::Stopped, ProbeEvent::Due { .. }) => {
                Err(TransitionError::Stale)
            }
            (ProbeState::Fired | ProbeState::Stopped, ProbeEvent::Run | ProbeEvent::Stop) => {
                Err(TransitionError::Domain(ProbeRefusal::AlreadyFinished))
            }
        }
    }
    fn transitions() -> &'static [Transition] {
        &[
            Transition {
                from: "waiting",
                event: "due",
                to: "fired",
                terminal: true,
            },
            Transition {
                from: "waiting",
                event: "run",
                to: "fired",
                terminal: true,
            },
            Transition {
                from: "waiting",
                event: "stop",
                to: "stopped",
                terminal: true,
            },
        ]
    }
    fn next_due(state: &ProbeState) -> Option<EngineTime> {
        match state {
            ProbeState::Waiting { due, .. } => *due,
            ProbeState::Fired | ProbeState::Stopped => None,
        }
    }
}
