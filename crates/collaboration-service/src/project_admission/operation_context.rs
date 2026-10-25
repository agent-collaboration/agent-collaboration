use super::ProjectAdmissionError;
use chrono::{DateTime, Utc};
use message_board::*;
use message_board_storage::{LinkMutation, ProjectWriteTransaction};

pub(super) struct OperationPlan {
    pub records: Vec<Record>,
    pub links: Vec<(TaskLink, LinkMutation)>,
    pub removed_subtasks: Vec<SubtaskId>,
    pub intents: Vec<(RecordRef, NotifyTarget)>,
    pub change: ChangeOutcome,
    pub rejection: Option<Vec<Refusal>>,
    pub conversation: Vec<ConversationMutation>,
    pub activated_memberships: Vec<RecordId>,
}
pub(super) enum ConversationMutation {
    Participant {
        thread: ThreadId,
        participant: Participant,
    },
    Watch {
        thread: ThreadId,
        reader: Identity,
        starts_after: FeedCursor,
        active: bool,
    },
    ProjectWatch {
        reader: Identity,
        starts_after: FeedCursor,
        active: bool,
    },
}
impl OperationPlan {
    pub fn changed(records: Vec<Record>) -> Self {
        Self {
            records,
            links: Vec::new(),
            removed_subtasks: Vec::new(),
            intents: Vec::new(),
            change: ChangeOutcome::Changed,
            rejection: None,
            conversation: Vec::new(),
            activated_memberships: Vec::new(),
        }
    }
    pub fn unchanged() -> Self {
        Self {
            change: ChangeOutcome::AlreadySo,
            ..Self::changed(Vec::new())
        }
    }
    pub fn rejected(reason: Refusal) -> Self {
        Self {
            rejection: Some(vec![reason]),
            ..Self::changed(Vec::new())
        }
    }
}

pub(super) struct OperationContext<'request> {
    pub request: &'request ProjectRequest,
    pub actor: RecordedActor,
    pub origin: ActorOrigin,
    pub now: DateTime<Utc>,
    pub high_water: FeedCursor,
    pub admitted_epoch: EpochNumber,
    pub scope: RecordScope,
    pub project: ProjectState,
    pub records: Vec<Record>,
    pub links: Vec<TaskLink>,
    pub live_subtasks: Vec<SubtaskId>,
    pub archived: bool,
    pub workspace_adrs: Vec<Adr>,
}

impl<'request> OperationContext<'request> {
    pub fn envelope(&self, id: RecordId) -> RecordEnvelope {
        RecordEnvelope {
            id,
            scope: self.scope.clone(),
            actor: self.actor.clone(),
            request: Some(self.request.request_id.clone()),
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor {
                    epoch: self.admitted_epoch,
                    position: self.high_water.position,
                },
                origin: self.origin.clone(),
            }),
            time: RecordTime::Router(self.now),
        }
    }
    pub fn record(&self, reference: &RecordRef) -> Result<&Record, Refusal> {
        self.records
            .iter()
            .find(|record| record.reference() == *reference)
            .ok_or_else(|| Refusal::NotFound(reference.clone()))
    }
    pub fn task(&self, id: &TaskId) -> Result<Task, Refusal> {
        match self.record(&id.clone().into())? {
            Record::Task(task) => Ok(task.clone()),
            _ => Err(Refusal::Task(TaskRefusal::NotFound {
                record: id.clone().into(),
            })),
        }
    }
    pub fn workstream(&self, id: &WorkstreamId) -> Result<Workstream, Refusal> {
        match self.record(&id.clone().into())? {
            Record::Workstream(workstream) => Ok(workstream.clone()),
            _ => Err(Refusal::Workstream(WorkstreamRefusal::NotFound {
                record: id.clone().into(),
            })),
        }
    }
    pub fn active_role(
        &self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Option<&RoleAssignment> {
        self.records.iter().find_map(|record| match record {
            Record::RoleAssignment(assignment)
                if assignment.subject() == *subject
                    && assignment.role() == role
                    && assignment.ended.is_none() =>
            {
                Some(assignment)
            }
            _ => None,
        })
    }
    pub fn require_lead(&self) -> Result<(), Refusal> {
        let lead = self.active_role(
            &RoleSubject::Project(self.request.project_id.clone()),
            AuthorityRole::Lead,
        );
        work_rules::require_lead(
            &self.request.actor,
            lead.and_then(|role| role.holder.declared_identity())
                .as_ref(),
        )
        .map_err(Refusal::Workstream)
    }
    pub fn check_revision(
        &self,
        expected: SubjectRevision,
        current: SubjectRevision,
    ) -> Result<(), Refusal> {
        if expected == current {
            Ok(())
        } else {
            Err(Refusal::StaleRevision { current })
        }
    }
    pub fn require_new(&self, reference: RecordRef) -> Result<(), Refusal> {
        if self
            .records
            .iter()
            .any(|record| record.reference() == reference)
        {
            Err(Refusal::AlreadyExists(reference))
        } else {
            Ok(())
        }
    }
    pub fn require_open_workstream(&self, id: &WorkstreamId) -> Result<Workstream, Refusal> {
        let row = self.workstream(id)?;
        if !matches!(row.state, WorkstreamState::Open {}) {
            return Err(Refusal::Workstream(WorkstreamRefusal::Closed {
                workstream: id.clone(),
            }));
        }
        Ok(row)
    }
    fn stand_down_gate(&self) -> Result<(), Refusal> {
        if has_unacknowledged_stand_down(&self.records, &self.request.actor) {
            return Err(Refusal::Role(RoleRefusal::StandDownUnacknowledged {}));
        }
        Ok(())
    }
    pub async fn plan(
        &mut self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<OperationPlan, ProjectAdmissionError> {
        if self.archived
            && !matches!(
                self.request.operation,
                BoardOperation::ThreadUnwatch { .. }
                    | BoardOperation::ProjectUnwatch { .. }
                    | BoardOperation::ThreadUnsubscribe { .. }
                    | BoardOperation::ReadMarkInitialize { .. }
                    | BoardOperation::ReadMarkAdvance { .. }
            )
        {
            return Ok(OperationPlan::rejected(Refusal::Archived));
        }
        if !matches!(
            self.request.operation,
            BoardOperation::RoleAcknowledgeCurrent { .. }
        ) && let Err(refusal) = self.stand_down_gate()
        {
            return Ok(OperationPlan::rejected(refusal));
        }
        let closed_write = matches!(
            self.request.operation,
            BoardOperation::WorkstreamCreate { .. }
                | BoardOperation::WorkstreamRename { .. }
                | BoardOperation::WorkstreamClose { .. }
                | BoardOperation::TaskCreate { .. }
                | BoardOperation::TaskUpdate { .. }
                | BoardOperation::TaskMoveWorkstream { .. }
                | BoardOperation::TaskSetStatus { .. }
                | BoardOperation::TaskLink { .. }
                | BoardOperation::TaskUnlink { .. }
                | BoardOperation::TaskAction { .. }
                | BoardOperation::SubtaskAdd { .. }
                | BoardOperation::SubtaskCheck { .. }
                | BoardOperation::SubtaskMove { .. }
                | BoardOperation::SubtaskRemove { .. }
                | BoardOperation::WorkTrailAdd { .. }
                | BoardOperation::WorkTrailRedact { .. }
                | BoardOperation::RoleGrant { .. }
                | BoardOperation::RoleRelease { .. }
                | BoardOperation::RoleReplaceLead { .. }
                | BoardOperation::ProjectPublishPlan { .. }
                | BoardOperation::ProjectClose { .. }
        );
        if self.project.closed_at.is_some() && closed_write {
            return Ok(OperationPlan::rejected(Refusal::Closed {
                subject: self.request.project_id.clone().into(),
            }));
        }
        let plan = match &self.request.operation {
            BoardOperation::NotificationCancel { .. } => Err(Refusal::InvalidShape {
                field: "operation".into(),
            }),
            BoardOperation::WorkstreamCreate { .. }
            | BoardOperation::WorkstreamRename { .. }
            | BoardOperation::WorkstreamClose { .. }
            | BoardOperation::WorkTrailAdd { .. }
            | BoardOperation::WorkTrailRedact { .. } => self.work_plan(),
            BoardOperation::TaskCreate { .. }
            | BoardOperation::TaskUpdate { .. }
            | BoardOperation::TaskMoveWorkstream { .. }
            | BoardOperation::TaskSetStatus { .. }
            | BoardOperation::TaskLink { .. }
            | BoardOperation::TaskUnlink { .. }
            | BoardOperation::TaskAction { .. }
            | BoardOperation::SubtaskAdd { .. }
            | BoardOperation::SubtaskCheck { .. }
            | BoardOperation::SubtaskMove { .. }
            | BoardOperation::SubtaskRemove { .. } => self.task_plan(),
            BoardOperation::RoleGrant { .. }
            | BoardOperation::RoleRelease { .. }
            | BoardOperation::RoleReplaceLead { .. }
            | BoardOperation::OwnerGrantRecord { .. }
            | BoardOperation::RoleAcknowledgeCurrent { .. } => self.role_plan(write).await?,
            BoardOperation::ProjectPublishPlan { .. }
            | BoardOperation::ProjectClose { .. }
            | BoardOperation::ProjectDescribe { .. } => self.project_lifecycle_plan(),
            BoardOperation::FactRecord { fact } => {
                if let Some(anchor) = fact.anchor
                    && write.cursor_validity(anchor).await?
                        != message_board_storage::CursorValidity::Valid
                {
                    return Ok(OperationPlan::rejected(Refusal::InvalidShape {
                        field: "anchor".into(),
                    }));
                }
                self.project_lifecycle_plan()
            }
            BoardOperation::ThreadCreate { .. }
            | BoardOperation::ThreadRefile { .. }
            | BoardOperation::ThreadPost { .. }
            | BoardOperation::ThreadResolve { .. }
            | BoardOperation::ThreadUnresolve { .. }
            | BoardOperation::ReadMarkInitialize { .. }
            | BoardOperation::ReadMarkAdvance { .. } => self.thread_plan(write).await?,
            BoardOperation::ThreadJoin { .. }
            | BoardOperation::ThreadLeave { .. }
            | BoardOperation::ThreadWatch { .. }
            | BoardOperation::ThreadUnwatch { .. }
            | BoardOperation::ProjectWatch { .. }
            | BoardOperation::ProjectUnwatch { .. }
            | BoardOperation::ThreadSubscribe { .. }
            | BoardOperation::ThreadUnsubscribe { .. }
            | BoardOperation::ThreadRenew { .. } => self.membership_plan(write).await?,
        };
        let mut plan = plan.unwrap_or_else(OperationPlan::rejected);
        for record in &plan.records {
            if let Err(error) = record.validate_bounds() {
                return Ok(OperationPlan::rejected(Refusal::InvalidShape {
                    field: error.field.to_owned(),
                }));
            }
        }
        self.materialize_covered_post_watches(write, &mut plan)
            .await?;
        Ok(plan)
    }

    pub async fn resolve_intents(
        &self,
        write: &mut ProjectWriteTransaction,
        targets: Vec<(RecordRef, NotifyTarget)>,
    ) -> Result<Vec<NotifyIntent>, ProjectAdmissionError> {
        let mut causes: Vec<(RecordRef, Vec<NotifyTarget>)> = Vec::new();
        for (cause, target) in targets {
            if let Some((_, targets)) = causes.iter_mut().find(|(existing, _)| *existing == cause) {
                targets.push(target);
            } else {
                causes.push((cause, vec![target]));
            }
        }
        let mut intents = Vec::new();
        for (cause, targets) in causes {
            let mut recipients = std::collections::BTreeMap::new();
            // Names win even when a role appeared earlier in the caller's target list.
            for target in &targets {
                if let NotifyTarget::Actor(identity) = target {
                    recipients
                        .entry(identity.clone())
                        .or_insert(ResolvedRecipient {
                            actor: RecordedActor::declared(identity.clone()),
                            via_role: None,
                        });
                }
            }
            let mut empty_roles = Vec::new();
            for target in targets {
                let NotifyTarget::Role(selector) = target else {
                    continue;
                };
                let subject = selector.subject();
                let role = selector.role();
                match crate::board_views::resolve(write, &selector).await? {
                    RoleResolution::Holders { actors, revision } => {
                        for actor in actors {
                            let identity = actor
                                .declared_identity()
                                .ok_or_else(|| {
                                    ProjectAdmissionError::InvalidRecord(
                                        "role holder is not a declared actor".into(),
                                    )
                                })?
                                .clone();
                            recipients.entry(identity).or_insert(ResolvedRecipient {
                                actor,
                                via_role: Some(RoleRecipient {
                                    subject: subject.clone(),
                                    role,
                                    assignment_revision: revision,
                                }),
                            });
                        }
                    }
                    RoleResolution::NoHolder { revision } => {
                        let recipient = IntentRecipient::NoHolder {
                            subject,
                            role,
                            assignment_revision: revision,
                        };
                        if !empty_roles.contains(&recipient) {
                            empty_roles.push(recipient);
                        }
                    }
                }
            }
            for recipient in recipients
                .into_values()
                .map(IntentRecipient::Resolved)
                .chain(empty_roles)
            {
                let state = match recipient {
                    IntentRecipient::Resolved(_) => IntentState::Emitted {},
                    IntentRecipient::NoHolder { .. } => IntentState::Refused {
                        reason: IntentRefusal::NoHolder {},
                    },
                };
                let intent_id = NotifyIntentId::generate();
                intents.push(
                    NotifyIntent::new(NotifyIntentProps {
                        envelope: self.envelope(intent_id.clone().into()),
                        intent_id,
                        cause: cause.clone(),
                        recipient,
                        state,
                    })
                    .map_err(|error| ProjectAdmissionError::InvalidRecord(error.to_string()))?,
                );
            }
        }
        Ok(intents)
    }
}

pub(super) fn has_unacknowledged_stand_down(records: &[Record], actor: &Identity) -> bool {
    records.iter().any(|record| {
        if let Record::RoleAssignment(role) = record
            && role.holder.declared_identity().as_ref() == Some(actor)
            && matches!(role.ended, Some(RoleEnd::StoodDown { .. }))
            && let Some(stood_down) = role.envelope.position.project_cursor()
        {
            !records.iter().any(|record| matches!(record, Record::StandDownAcknowledgement(ack) if ack.assignment == role.assignment_id && ack.envelope.actor.declared_identity().as_ref() == Some(actor) && ack.read_through.position >= stood_down.position))
        } else {
            false
        }
    })
}
