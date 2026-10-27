//! The sole project-domain write path: lease, schema, replay, rules, atomic effects.
use crate::{FleetClient, FleetClientError, ProjectRegistryError, ProjectStoreRegistry};
use chrono::Utc;
use message_board::{
    ActorOrigin, BoardOperation, MachineLabel, MailItemRef, NumericValidationError, ProjectRequest,
    Record, RequestLedgerEntry, RequestResult, ServiceId, StoredDecision, SubscriptionMembership,
    SubscriptionMembershipProps, WriteReply, request_canonical,
};
use message_board_storage::{ProjectStorageError, ProjectWriteTransaction};
use std::sync::Arc;

mod archive_observation;
#[cfg(test)]
mod behavior_tests;
pub use archive_observation::ProjectArchiveObservation;
mod follow_admission;
mod follow_coverage_validation;
mod intent_admission;
mod intent_handoff;
mod mail_frontier_admission;
pub(crate) use follow_admission::FollowBatchProposal;
mod intent_re_resolution;
mod operation_context;
mod operation_operand_reads;
pub use intent_admission::IntentReply;
pub use intent_handoff::{IntentHandoffInput, IntentRecipientEnrollment};
mod membership_expiry;
mod membership_mail_cancels;
pub(crate) use membership_expiry::MembershipExpiryProposal;
mod membership_operations;
mod participation_operations;
mod project_creation;
mod project_lifecycle_operations;
mod project_rejection;
mod read_mark_admission;
mod reader_state;
mod receipt_reads;
mod request_gate;
mod role_operations;
mod task_operations;
mod thread_operations;
mod thread_subject_operations;
mod work_operations;
mod write_gate;
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
    pub(crate) installation: ServiceId,
    pub(crate) machine: MachineLabel,
}

pub struct ProjectAdmissionProps {
    pub registry: Arc<ProjectStoreRegistry>,
    pub fleet: Arc<FleetClient>,
    pub identity: crate::MachineIdentity,
}

#[cfg(feature = "runtime-test-support")]
pub struct ProjectAdmissionTestContext {
    pub fleet: Arc<FleetClient>,
    pub registry: Arc<ProjectStoreRegistry>,
    pub origin: ActorOrigin,
}

impl ProjectAdmission {
    #[must_use]
    pub fn project_registry(&self) -> Arc<ProjectStoreRegistry> {
        self.registry.clone()
    }
    #[cfg(feature = "runtime-test-support")]
    #[must_use]
    pub fn runtime_test_context(&self) -> ProjectAdmissionTestContext {
        ProjectAdmissionTestContext {
            fleet: self.fleet.clone(),
            registry: self.registry.clone(),
            origin: self.record_origin(),
        }
    }

    pub(crate) fn record_origin(&self) -> ActorOrigin {
        ActorOrigin {
            installation: self.installation.clone(),
            machine: self.machine.clone(),
        }
    }

    pub fn new(props: ProjectAdmissionProps) -> Result<Self, ProjectAdmissionError> {
        Ok(Self {
            registry: props.registry,
            fleet: props.fleet,
            installation: ServiceId::try_from(String::from(props.identity.service_id().clone()))
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
            machine: MachineLabel::try_from(props.identity.machine_label().as_str().to_owned())
                .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
        })
    }

    #[must_use]
    pub fn subscription_window_store(
        &self,
        interactions: Arc<interaction_storage::InteractionStore>,
    ) -> crate::SubscriptionWindowStore {
        crate::SubscriptionWindowStore::new(self.registry.clone(), interactions)
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
        self.submit_with_origin(request, self.record_origin()).await
    }

    /// The machine-session handler supplies central's verified record provenance.
    pub async fn submit_forwarded_post(
        &self,
        request: &ProjectRequest,
        origin: ActorOrigin,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        if !matches!(
            request.operation,
            BoardOperation::ThreadCreate { .. } | BoardOperation::ThreadPost { .. }
        ) {
            return Err(ProjectAdmissionError::InvalidRecord(
                "forwarded post operation".to_owned(),
            ));
        }
        self.submit_with_origin(request, origin).await
    }

    async fn submit_with_origin(
        &self,
        request: &ProjectRequest,
        origin: ActorOrigin,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        if matches!(request.operation, BoardOperation::NotificationCancel { .. }) {
            return Err(ProjectAdmissionError::InvalidRecord(
                "requested notification cancel requires its read-only admission".into(),
            ));
        }
        let result = self.decide(request, origin).await?;
        let reader = self.reader_after_result(request, &result).await?;
        Ok(WriteReply { result, reader })
    }

    async fn decide(
        &self,
        request: &ProjectRequest,
        origin: ActorOrigin,
    ) -> Result<RequestResult, ProjectAdmissionError> {
        self.decide_at(request, None, origin)
            .await
            .or_else(write_gate::refusal_result)
    }

    #[cfg(test)]
    pub(crate) async fn submit_at(
        &self,
        request: &ProjectRequest,
        now: chrono::DateTime<Utc>,
    ) -> Result<WriteReply, ProjectAdmissionError> {
        let result = self
            .decide_at(request, Some(now), self.record_origin())
            .await
            .or_else(write_gate::refusal_result)?;
        let reader = self.reader_after_result(request, &result).await?;
        Ok(WriteReply { result, reader })
    }

    async fn decide_at(
        &self,
        request: &ProjectRequest,
        fixture_time: Option<chrono::DateTime<Utc>>,
        origin: ActorOrigin,
    ) -> Result<RequestResult, ProjectAdmissionError> {
        let hash = request_canonical::canonical_hash(request)?;
        let ready = self
            .begin_request(
                request_gate::RequestGateInput {
                    project: &request.project_id,
                    request: &request.request_id,
                    hash: &hash,
                },
                origin,
            )
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
            accepted_in: Some(epoch),
            decision: decision.clone(),
        };
        let reply = write.request_decision(&ledger).await?;
        write.append_record(Record::Request(ledger)).await?;
        write.commit().await?;
        self.registry.notify_activity(&request.project_id);
        Ok(RequestResult::Decided(reply))
    }
}

async fn apply_plan(
    write: &mut ProjectWriteTransaction,
    context: &OperationContext<'_>,
    plan: OperationPlan,
) -> Result<StoredDecision, ProjectAdmissionError> {
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
        if let Record::SubscriptionMembership(member) = &record {
            membership_mail_cancels::record_membership_cancel(write, member).await?;
        }
        references.push(record.reference());
        let cursor = write.append_record(record).await?;
        positions.push(write.record_placement(cursor));
    }
    for (link, mutation) in plan.links {
        write.edit_task_link(&link, mutation).await?;
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
        let cursor = write.append_record(Record::NotifyIntent(intent)).await?;
        positions.push(write.record_placement(cursor));
    }
    Ok(match plan.rejection {
        Some(reasons) => StoredDecision::Rejected { reasons },
        None => {
            StoredDecision::accepted(references.into_iter().zip(positions).collect(), plan.change)
        }
    })
}
