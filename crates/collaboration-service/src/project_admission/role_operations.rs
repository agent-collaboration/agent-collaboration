use super::{
    ProjectAdmissionError,
    operation_context::{OperationContext, OperationPlan},
};
use message_board::*;
use message_board_storage::ProjectWriteTransaction;

impl OperationContext<'_> {
    pub(super) async fn role_plan(
        &self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<Result<OperationPlan, Refusal>, ProjectAdmissionError> {
        match &self.request.operation {
            BoardOperation::OwnerGrantRecord { grant_id, grant } => {
                let GrantKind::ReplaceLead { project, .. } = grant;
                if *project != self.request.project_id {
                    return Ok(Err(Refusal::InvalidShape {
                        field: "grant.project".into(),
                    }));
                }
                if let Err(reason) = self.require_new(grant_id.clone().into()) {
                    return Ok(Err(reason));
                }
                Ok(Ok(OperationPlan::changed(vec![Record::OwnerGrant(
                    OwnerGrant {
                        envelope: self.envelope(grant_id.clone().into()),
                        id: grant_id.clone(),
                        grant: grant.clone(),
                        provenance: OwnerWordsProvenance::Relayed {
                            by: self.actor.clone(),
                        },
                    },
                )])))
            }
            BoardOperation::RoleAcknowledgeCurrent { read_through } => {
                if read_through.epoch != self.high_water.epoch
                    || read_through.position > self.high_water.position
                {
                    return Ok(Err(Refusal::InvalidShape {
                        field: "readThrough".into(),
                    }));
                }
                let mut records = Vec::new();
                for record in &self.records {
                    if let Record::RoleAssignment(row) = record
                        && row.holder.declared_identity() == Some(&self.request.actor)
                        && matches!(row.ended, Some(RoleEnd::StoodDown { .. }))
                        && let RecordPosition::Project(stood_down) = row.envelope.position
                    {
                        if read_through.epoch != stood_down.epoch
                            || read_through.position < stood_down.position
                        {
                            return Ok(Err(Refusal::Role(RoleRefusal::StandDownUnacknowledged {})));
                        }
                        let existing = self.records.iter().any(|record| matches!(record, Record::StandDownAcknowledgement(ack) if ack.assignment == row.assignment_id && ack.envelope.actor.declared_identity() == Some(&self.request.actor) && ack.read_through.epoch == read_through.epoch && ack.read_through.position >= read_through.position));
                        if !existing {
                            records.push(Record::StandDownAcknowledgement(
                                StandDownAcknowledgement {
                                    envelope: self.envelope(RecordId::generate()),
                                    assignment: row.assignment_id.clone(),
                                    read_through: *read_through,
                                },
                            ));
                        }
                    }
                }
                Ok(Ok(if records.is_empty() {
                    OperationPlan::unchanged()
                } else {
                    OperationPlan::changed(records)
                }))
            }
            BoardOperation::RoleGrant {
                subject,
                role,
                holder,
                model,
                lineage,
                expected_revision,
                owner_grant,
            } => {
                let current = write.assignment_revision(subject).await?;
                let holder = RecordedActor {
                    actor: message_board::ActorKind::Declared(holder.clone()),
                    ..self.actor.clone()
                };
                Ok(self.grant_plan(RoleGrantPlan {
                    subject,
                    role: *role,
                    holder,
                    model: model.clone(),
                    lineage: lineage.clone(),
                    expected: *expected_revision,
                    current,
                    owner_grant: owner_grant.as_ref(),
                    replacement_reason: None,
                }))
            }
            BoardOperation::RoleReplaceLead {
                new_holder,
                owner_grant,
                reason,
                expected_assignment_revision,
            } => {
                let subject = RoleSubject::Project(self.request.project_id.clone());
                let current = write.assignment_revision(&subject).await?;
                Ok(self.grant_plan(RoleGrantPlan {
                    subject: &subject,
                    role: AuthorityRole::Lead,
                    holder: new_holder.clone(),
                    model: None,
                    lineage: None,
                    expected: *expected_assignment_revision,
                    current,
                    owner_grant: Some(owner_grant),
                    replacement_reason: Some(reason.clone()),
                }))
            }
            BoardOperation::RoleRelease {
                subject,
                role,
                holder,
                expected_revision,
            } => {
                let current = write.assignment_revision(subject).await?;
                if current != *expected_revision {
                    return Ok(Err(assignment_stale(current)?));
                }
                if let Err(reason) = self
                    .validate_role_subject(subject, *role)
                    .and_then(|()| self.require_role_grant(subject, *role))
                {
                    return Ok(Err(reason));
                }
                if (role.cardinality() == RoleCardinality::Many && holder.is_none())
                    || holder
                        .as_ref()
                        .is_some_and(|holder| holder.declared_identity().is_none())
                {
                    return Ok(Err(Refusal::InvalidShape {
                        field: "holder".into(),
                    }));
                }
                let active = self.records.iter().find_map(|record| match record {
                    Record::RoleAssignment(row)
                        if row.subject() == *subject
                            && row.role() == *role
                            && row.ended.is_none()
                            && holder.as_ref().is_none_or(|holder| row.holder == *holder) =>
                    {
                        Some(row)
                    }
                    _ => None,
                });
                let Some(mut row) = active.cloned() else {
                    return Ok(Ok(OperationPlan::unchanged()));
                };
                row.ended = Some(RoleEnd::Released {});
                row.envelope = self.envelope(row.assignment_id.clone().into());
                Ok(Ok(OperationPlan::changed(vec![Record::RoleAssignment(
                    row,
                )])))
            }
            _ => Ok(Err(Refusal::InvalidShape {
                field: "operation".into(),
            })),
        }
    }

    fn require_role_grant(
        &self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Result<(), Refusal> {
        let lead = self.active_role(
            &RoleSubject::Project(self.request.project_id.clone()),
            AuthorityRole::Lead,
        );
        let implementer = self.active_role(subject, AuthorityRole::Implementer);
        work_rules::require_role_grant(
            &self.request.actor,
            lead.and_then(|row| row.holder.declared_identity()),
            implementer.and_then(|row| row.holder.declared_identity()),
            role,
        )
        .map_err(Refusal::Role)
    }
    fn validate_role_subject(
        &self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Result<(), Refusal> {
        match (subject, role) {
            (RoleSubject::Project(project), AuthorityRole::Lead | AuthorityRole::Advisor)
                if *project == self.request.project_id =>
            {
                Ok(())
            }
            (
                RoleSubject::Workstream(workstream),
                AuthorityRole::Implementer
                | AuthorityRole::Verifier
                | AuthorityRole::Reviewer
                | AuthorityRole::Operator,
            ) => self.workstream(workstream).map(|_| ()),
            _ => Err(Refusal::InvalidShape {
                field: "subject".into(),
            }),
        }
    }
    fn owner_grant_for(
        &self,
        grant_id: &OwnerGrantId,
        holder: &RecordedActor,
    ) -> Result<&OwnerGrant, Refusal> {
        let Record::OwnerGrant(grant) = self.record(&grant_id.clone().into())? else {
            return Err(Refusal::NotFound(grant_id.clone().into()));
        };
        let GrantKind::ReplaceLead {
            project,
            new_holder,
            ..
        } = &grant.grant;
        if project != &self.request.project_id || new_holder != holder {
            return Err(Refusal::InvalidShape {
                field: "ownerGrant".into(),
            });
        }
        if self.records.iter().any(|record| matches!(record, Record::RoleAssignment(row) if row.owner_grant.as_ref() == Some(grant_id))) { return Err(Refusal::InvalidShape { field: "ownerGrant".into() }); }
        Ok(grant)
    }
    fn grant_plan(&self, input: RoleGrantPlan<'_>) -> Result<OperationPlan, Refusal> {
        self.validate_role_subject(input.subject, input.role)?;
        if input.current != input.expected {
            return Err(Refusal::StaleRevision {
                current: SubjectRevision::try_from(input.current.get()).map_err(|_| {
                    Refusal::InvalidShape {
                        field: "revision".into(),
                    }
                })?,
            });
        }
        let active = self.active_role(input.subject, input.role);
        let mut holder = input.holder;
        let mut grant_used = None;
        let mut stood_down_reason = input.replacement_reason;
        if input.role == AuthorityRole::Lead {
            if stood_down_reason.is_none()
                && let Some(active) = active
            {
                self.require_role_grant(input.subject, AuthorityRole::Lead)?;
                return if active.holder.actor == holder.actor {
                    Ok(OperationPlan::unchanged())
                } else {
                    Err(Refusal::InvalidShape {
                        field: "role".into(),
                    })
                };
            }
            let recent = self
                .records
                .iter()
                .filter_map(|record| match record {
                    Record::RoleAssignment(row)
                        if row.subject() == *input.subject && row.role() == AuthorityRole::Lead =>
                    {
                        Some(row)
                    }
                    _ => None,
                })
                .max_by_key(|row| match row.envelope.position {
                    RecordPosition::Project(cursor) => cursor.position.get(),
                    RecordPosition::Fleet(_) | RecordPosition::Unsequenced => 0,
                });
            let needs_grant = match recent {
                Some(row) => row.holder.actor != holder.actor,
                None => holder.declared_identity() != Some(&self.request.actor),
            };
            if needs_grant || input.owner_grant.is_some() {
                let grant_id = input.owner_grant.ok_or_else(|| Refusal::InvalidShape {
                    field: "ownerGrant".into(),
                })?;
                let grant = self.owner_grant_for(grant_id, &holder)?;
                let GrantKind::ReplaceLead {
                    new_holder, reason, ..
                } = &grant.grant;
                holder = new_holder.clone();
                if stood_down_reason.is_none() {
                    stood_down_reason = Some(reason.clone());
                }
                grant_used = Some(grant_id.clone());
            } else if recent.is_some()
                && !(active.is_none()
                    && holder.declared_identity() == Some(&self.request.actor)
                    && recent.is_some_and(|row| row.holder.actor == holder.actor))
            {
                self.require_role_grant(input.subject, input.role)?;
            }
        } else {
            self.require_role_grant(input.subject, input.role)?;
            if matches!(input.subject, RoleSubject::Workstream(_)) {
                let current_holder = |role| {
                    self.active_role(input.subject, role)
                        .and_then(|row| row.holder.declared_identity())
                };
                let implementer = if input.role == AuthorityRole::Implementer {
                    holder.declared_identity()
                } else {
                    current_holder(AuthorityRole::Implementer)
                };
                let verifier = if input.role == AuthorityRole::Verifier {
                    holder.declared_identity()
                } else {
                    current_holder(AuthorityRole::Verifier)
                };
                let mut reviewers: Vec<_> = self
                    .records
                    .iter()
                    .filter_map(|record| match record {
                        Record::RoleAssignment(row)
                            if row.subject() == *input.subject
                                && row.role() == AuthorityRole::Reviewer
                                && row.ended.is_none() =>
                        {
                            row.holder.declared_identity()
                        }
                        _ => None,
                    })
                    .collect();
                if input.role == AuthorityRole::Reviewer
                    && let Some(reviewer) = holder.declared_identity()
                {
                    reviewers.push(reviewer);
                }
                work_rules::distinct_workstream_roles(implementer, verifier, &reviewers)
                    .map_err(Refusal::Role)?;
            }
        }
        if input.role.cardinality() == RoleCardinality::Many && self.records.iter().any(|record| matches!(record, Record::RoleAssignment(row) if row.subject() == *input.subject && row.role() == input.role && row.ended.is_none() && row.holder.actor == holder.actor)) {
            return Ok(OperationPlan::unchanged());
        }
        if active.is_some_and(|row| {
            row.holder.actor == holder.actor
                && row.model == input.model
                && row.lineage == input.lineage
        }) {
            return Ok(OperationPlan::unchanged());
        }
        let mut records = Vec::new();
        if input.role.cardinality() == RoleCardinality::One
            && let Some(mut ended) = active.cloned()
        {
            ended.ended = Some(match stood_down_reason {
                Some(reason) if ended.holder.actor != holder.actor => RoleEnd::StoodDown { reason },
                _ => RoleEnd::Replaced {},
            });
            ended.envelope = self.envelope(ended.assignment_id.clone().into());
            records.push(Record::RoleAssignment(ended));
        }
        let assignment_id = RoleAssignmentId::generate();
        let assigned = RoleAssignment::new(RoleAssignmentProps {
            envelope: self.envelope(assignment_id.clone().into()),
            assignment_id: assignment_id.clone(),
            subject: input.subject.clone(),
            role: input.role,
            holder: holder.clone(),
            granted_by: self.actor.clone(),
            model: input.model,
            lineage: input.lineage,
            ended: None,
            owner_grant: grant_used,
        })
        .map_err(|_| Refusal::InvalidShape {
            field: "role".into(),
        })?;
        records.push(Record::RoleAssignment(assigned));
        let mut plan = OperationPlan::changed(records);
        plan.intents.push((
            assignment_id.into(),
            NotifyTarget::Actor(
                holder
                    .declared_identity()
                    .ok_or_else(|| Refusal::InvalidShape {
                        field: "holder".into(),
                    })?
                    .clone(),
            ),
        ));
        Ok(plan)
    }
}
struct RoleGrantPlan<'subject> {
    subject: &'subject RoleSubject,
    role: AuthorityRole,
    holder: RecordedActor,
    model: Option<ResourceName>,
    lineage: Option<ResourceName>,
    expected: AssignmentRevision,
    current: AssignmentRevision,
    owner_grant: Option<&'subject OwnerGrantId>,
    replacement_reason: Option<String>,
}
fn assignment_stale(current: AssignmentRevision) -> Result<Refusal, ProjectAdmissionError> {
    Ok(Refusal::StaleRevision {
        current: SubjectRevision::try_from(current.get())?,
    })
}
