//! The sole project-domain write path: lease, schema, replay, rules, atomic effects.
use crate::{
    FleetClient, FleetClientError, GrantResult, LocalLeaseAuthority, ProjectRegistryError,
    ProjectStoreRegistry,
};
use chrono::Utc;
use message_board::*;
use message_board_storage::{ProjectStorageError, ProjectWriteTransaction};
use std::{sync::Arc, time::Instant};

#[cfg(test)]
mod behavior_tests;
mod intent_admission;
mod intent_re_resolution;
mod operation_context;
mod operation_operand_reads;
pub use intent_admission::IntentReply;
mod membership_operations;
mod participation_operations;
mod process_operations;
mod project_checks;
mod project_creation;
mod read_mark_admission;
mod reader_state;
mod recorded_move_check;
mod request_gate;
mod role_operations;
mod task_operations;
mod thread_operations;
mod thread_subject_operations;
mod work_operations;
use operation_context::{ConversationMutation, OperationContext, OperationPlan};

#[derive(Debug, thiserror::Error)]
pub enum ProjectAdmissionError {
    #[error(transparent)]
    Registry(#[from] ProjectRegistryError),
    #[error(transparent)]
    Storage(#[from] ProjectStorageError),
    #[error(transparent)]
    Fleet(#[from] FleetClientError),
    #[error(transparent)]
    Canonical(#[from] request_canonical::CanonicalRequestError),
    #[error(transparent)]
    Numeric(#[from] NumericValidationError),
    #[error("a constructed project record is invalid: {0}")]
    InvalidRecord(String),
}

pub struct ProjectAdmission {
    pub(crate) registry: Arc<ProjectStoreRegistry>,
    pub(crate) fleet: Arc<FleetClient>,
    pub(crate) leases: Arc<LocalLeaseAuthority>,
    pub(crate) installation: ServiceId,
    pub(crate) machine: MachineLabel,
}

pub struct ProjectAdmissionProps {
    pub registry: Arc<ProjectStoreRegistry>,
    pub fleet: Arc<FleetClient>,
    pub leases: Arc<LocalLeaseAuthority>,
    pub identity: crate::MachineIdentity,
}

impl ProjectAdmission {
    pub fn new(props: ProjectAdmissionProps) -> Result<Self, ProjectAdmissionError> {
        Ok(Self {
            registry: props.registry,
            fleet: props.fleet,
            leases: props.leases,
            installation: ServiceId::try_from(String::from(props.identity.service_id().clone()))
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
            machine: MachineLabel::try_from(props.identity.machine_label().as_str().to_owned())
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
        })
    }

    #[must_use]
    pub fn subscription_window_store(
        &self,
        automation: Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    ) -> crate::SubscriptionWindowStore {
        crate::SubscriptionWindowStore::new(self.registry.clone(), automation)
    }
    pub async fn close(self) -> Result<(), ProjectAdmissionError> {
        let registry = Arc::try_unwrap(self.registry).map_err(|_| {
            ProjectAdmissionError::InvalidRecord(
                "project registry still owned during shutdown".to_owned(),
            )
        })?;
        registry.close().await?;
        let fleet = Arc::try_unwrap(self.fleet).map_err(|_| {
            ProjectAdmissionError::InvalidRecord(
                "fleet client still owned during shutdown".to_owned(),
            )
        })?;
        fleet.close().await;
        Ok(())
    }
    pub async fn submit(
        &self,
        request: &ProjectRequest,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let result = self.decide(request).await?;
        let reader = self.reader_state(request).await?;
        Ok(WriteReply { result, reader })
    }

    async fn decide(
        &self,
        request: &ProjectRequest,
    ) -> Result<RequestResult, ProjectAdmissionError> {
        self.decide_at(request, None).await
    }

    #[cfg(test)]
    pub(crate) async fn submit_at(
        &self,
        request: &ProjectRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let result = self.decide_at(request, Some(now)).await?;
        let reader = self.reader_state(request).await?;
        Ok(WriteReply { result, reader })
    }

    async fn decide_at(
        &self,
        request: &ProjectRequest,
        fixture_time: Option<chrono::DateTime<Utc>>,
    ) -> Result<RequestResult, ProjectAdmissionError> {
        let hash = request_canonical::canonical_hash(request)?;
        let ready = self
            .begin_request(request_gate::RequestGateInput {
                project: &request.project_id,
                request: &request.request_id,
                observed_epoch: request.observed_epoch,
                hash: &hash,
            })
            .await?;
        let request_gate::ReadyRequest {
            mut write,
            epoch,
            high_water,
        } = match ready {
            request_gate::RequestGate::Ready(ready) => *ready,
            request_gate::RequestGate::Reply(reply) => return Ok(reply),
        };
        let now = fixture_time.unwrap_or_else(Utc::now);
        let mut context =
            OperationContext::load(self, request, &mut write, now, high_water).await?;
        let plan = context.plan(&mut write).await?;
        let decision = apply_plan(&mut write, &context, plan).await?;
        let ledger = RequestLedgerEntry {
            envelope: context.envelope(request.request_id.clone().into()),
            request_id: request.request_id.clone(),
            canonical_hash: hash,
            observed_epoch: request.observed_epoch,
            accepted_in: Some(epoch),
            decision: decision.clone(),
            resubmits: request.resubmits.clone(),
        };
        write.append_record(Record::Request(ledger)).await?;
        write.commit().await?;
        self.registry.notify_activity(&request.project_id);
        Ok(RequestResult::Decided(decision))
    }
}

fn granted_epoch(grant: GrantResult) -> Result<EpochNumber, RequestResult> {
    match grant {
        GrantResult::Holder { epoch, deadline } if Instant::now() < deadline => Ok(epoch),
        GrantResult::Holder { .. } => Err(RequestResult::LeaseExpired {
            expired_at: Utc::now(),
        }),
        GrantResult::Expired { expired_at } => Err(RequestResult::LeaseExpired { expired_at }),
        GrantResult::NotHolder { current } => Err(RequestResult::NotWriter { current }),
        GrantResult::Unavailable => Err(RequestResult::NotWriter { current: None }),
    }
}

async fn apply_plan(
    write: &mut ProjectWriteTransaction,
    context: &OperationContext<'_>,
    plan: OperationPlan,
) -> Result<RequestDecision, ProjectAdmissionError> {
    let mut references = Vec::new();
    let mut positions = Vec::new();
    let intents = context.resolve_intents(write, plan.intents).await?;
    let activation_position = if plan.activated_memberships.is_empty() {
        None
    } else {
        let count = plan
            .records
            .len()
            .checked_add(intents.len())
            .and_then(|count| count.checked_add(1))
            .ok_or_else(|| {
                ProjectAdmissionError::InvalidRecord("activation position overflow".to_owned())
            })?;
        Some(
            context
                .next_cursor(u64::try_from(count).map_err(|_| {
                    ProjectAdmissionError::InvalidRecord("activation position overflow".to_owned())
                })?)
                .map_err(|reason| {
                    ProjectAdmissionError::InvalidRecord(format!("activation position: {reason:?}"))
                })?,
        )
    };
    for mut record in plan.records {
        if let Record::ReadMark(mark) = record {
            write.write_reader_mark(mark).await?;
            continue;
        }
        if let Record::SubscriptionMembership(member) = &mut record
            && plan.activated_memberships.contains(&member.envelope.id)
            && let Some(position) = activation_position
        {
            let mut props: SubscriptionMembershipProps = member.clone().into();
            props.activated_at = position;
            *member = SubscriptionMembership::new(props)
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?;
        }
        references.push(record.reference());
        positions.push(RecordPosition::Project(write.append_record(record).await?));
    }
    for (link, mutation) in plan.links {
        write.edit_task_link(&link, mutation).await?;
    }
    for subtask in plan.removed_subtasks {
        write.remove_subtask(&subtask).await?;
    }
    for mutation in plan.conversation {
        match mutation {
            ConversationMutation::Participant {
                thread,
                participant,
            } => write.put_participant(&thread, &participant).await?,
            ConversationMutation::Watch {
                thread,
                reader,
                starts_after,
                active,
            } => {
                write
                    .set_thread_watch(&thread, &reader, starts_after, active)
                    .await?
            }
            ConversationMutation::ProjectWatch {
                reader,
                starts_after,
                active,
            } => {
                write
                    .set_project_watch(&reader, starts_after, active)
                    .await?
            }
        }
    }
    for intent in intents {
        references.push(intent.intent_id.clone().into());
        positions.push(RecordPosition::Project(
            write.append_record(Record::NotifyIntent(intent)).await?,
        ));
    }
    Ok(match plan.rejection {
        Some(reasons) => RequestDecision::Rejected { reasons },
        None => RequestDecision::Accepted {
            records: references,
            positions,
            change: plan.change,
        },
    })
}
