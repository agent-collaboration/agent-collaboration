#![deny(clippy::wildcard_enum_match_arm)]
pub use crate::engine::{
    receipts::{EngineReceipt, EngineRefusal},
    requests::EngineRequestOutcome,
};
use central_engine_model::{
    Context, EngineTime, EventTag, Machine, MachineKind, RefusalTag, StateTag, Transition,
    TransitionError,
};
use serde::{Deserialize, Serialize};

pub struct EngineProbe;
pub use central_engine_model::{InstanceKey, ItemFailureClass};
#[cfg(test)]
#[path = "engine_probe_test.rs"]
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SeedProbe {
    pub key: uuid::Uuid,
    pub group: uuid::Uuid,
    pub due: Option<EngineTime>,
    pub claim: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProbeRequest {
    pub request_id: workspace_model::RequestId,
    pub key: central_engine_model::InstanceKey,
    pub event: ProbeEvent,
}

impl<Host: crate::ObjectHost, Stores: crate::WorkspaceStoreCalls>
    crate::WorkspaceRuntime<Host, Stores>
{
    pub async fn seed_probe(
        &self,
        seed: SeedProbe,
    ) -> Result<central_storage::BatchOutcome, central_storage::StoreFailure> {
        use crate::{
            central_mail_store::timestamp,
            guarded_statements::{any, exact, text},
            workspace_decode_error::corrupt,
        };
        if !self.alarm_organization_is_provisioned().await? {
            return Err(corrupt("organization_identity.id"));
        }
        if seed.key.get_version_num() != 7 || seed.group.get_version_num() != 7 {
            return Err(corrupt("engine_probe_items.id"));
        }
        if seed.claim.is_empty()
            || seed
                .due
                .is_some_and(|time| !central_engine_model::supported_time(time))
        {
            return Err(corrupt("engine_probe_items.seed"));
        }
        let now = timestamp(self.now()?);
        self.host.run_batch(central_storage::StatementBatch {
            statements:vec![
                any("INSERT INTO engine_probe_groups(group_id,effect_count,created_at,updated_at) VALUES (?,0,?,?) ON CONFLICT(group_id) DO NOTHING",vec![text(seed.group),text(&now),text(&now)]),
                exact("INSERT INTO engine_probe_items(item_id,machine_kind,group_id,due_at,claim,state,completed_claim,send_id,connection_id,created_at,updated_at) VALUES (?,'engine_probe',?,?,?,'waiting',NULL,NULL,NULL,?,?)",
                    vec![text(seed.key),text(seed.group),seed.due.map_or(central_storage::SqlValue::Null,|due|text(timestamp(due))),text(seed.claim),text(&now),text(&now)]),
            ],alarm:central_storage::AlarmUpdate::Unchanged,
        }).await
    }

    pub async fn request_probe(
        &self,
        caller: &crate::WorkspaceCaller,
        request: ProbeRequest,
    ) -> Result<EngineRequestOutcome<ProbeState, ProbeRefusal>, central_storage::StoreFailure> {
        if !self.alarm_organization_is_provisioned().await? {
            return Err(crate::workspace_decode_error::corrupt(
                "organization_identity.id",
            ));
        }
        if let Err(reason) = self.admit_workspace(caller, None).await? {
            return Ok(EngineRequestOutcome::NotPermitted { reason });
        }
        let identity = match &caller.caller {
            workspace_model::Caller::Session(session) => workspace_model::Identity::AgentSession {
                session: session.clone(),
            },
            workspace_model::Caller::Person { person_id, .. } => {
                workspace_model::Identity::Person {
                    person_id: person_id.clone(),
                }
            }
        };
        let actor = self.resolve_identity(&identity).await?;
        self.request_engine::<EngineProbe, _>(
            &crate::engine::probe_store::ProbeStore(self),
            actor,
            request.request_id,
            request.key,
            request.event,
        )
        .await
    }
}
