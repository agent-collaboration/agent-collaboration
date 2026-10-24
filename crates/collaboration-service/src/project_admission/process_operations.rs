use super::{
    operation_context::{OperationContext, OperationPlan},
    work_operations::next_revision,
};
use message_board::*;

impl OperationContext<'_> {
    pub(super) fn process_plan(&self) -> Result<OperationPlan, Refusal> {
        match &self.request.operation {
            BoardOperation::ProjectDescribe {
                description,
                expected_revision,
            } => {
                self.check_revision(*expected_revision, self.project.revision)?;
                let mut project = self.project.clone();
                project.description = description.clone();
                project.revision = next_revision(project.revision)?;
                project.envelope = self.envelope(project.project_id.clone().into());
                Ok(OperationPlan::changed(vec![Record::Project(project)]))
            }
            BoardOperation::ProjectPinDefinition {
                definition,
                expected_revision,
            } => {
                self.require_lead()?;
                self.check_revision(*expected_revision, self.project.revision)?;
                if let Some(pinned) = &self.project.pinned_definition {
                    return if pinned == definition {
                        Ok(OperationPlan::unchanged())
                    } else {
                        Err(Refusal::DefinitionConflict)
                    };
                }
                let Some(registered) = self
                    .definitions
                    .iter()
                    .find(|row| row.reference == *definition)
                else {
                    return Err(Refusal::InvalidShape {
                        field: "definition".into(),
                    });
                };
                let mut project = self.project.clone();
                project.pinned_definition = Some(definition.clone());
                project.status = Some(registered.initial_status.clone());
                project.revision = next_revision(project.revision)?;
                project.envelope = self.envelope(self.request.project_id.clone().into());
                Ok(OperationPlan::changed(vec![Record::Project(project)]))
            }
            BoardOperation::ProjectPublishPlan {
                hash,
                document,
                expected_revision,
            } => {
                self.require_lead()?;
                self.check_revision(*expected_revision, self.project.revision)?;
                let current = self
                    .records
                    .iter()
                    .filter_map(|record| match record {
                        Record::PlanRevision(row) if row.superseded_by.is_none() => Some(row),
                        _ => None,
                    })
                    .max_by_key(|row| row.revision);
                if current.is_some_and(|row| row.hash == *hash && row.document == *document) {
                    return Ok(OperationPlan::unchanged());
                }
                let mut records = Vec::new();
                if let Some(mut previous) = current.cloned() {
                    previous.superseded_by = Some(hash.clone());
                    previous.envelope = self.envelope(previous.envelope.id.clone());
                    records.push(Record::PlanRevision(previous));
                }
                let mut project = self.project.clone();
                project.revision = next_revision(project.revision)?;
                records.push(Record::PlanRevision(PlanRevision {
                    envelope: self.envelope(RecordId::generate()),
                    hash: hash.clone(),
                    document: document.clone(),
                    superseded_by: None,
                    revision: project.revision,
                }));
                project.envelope = self.envelope(project.project_id.clone().into());
                records.push(Record::Project(project));
                Ok(OperationPlan::changed(records))
            }
            BoardOperation::ProjectClose {
                reason,
                expected_revision,
            } => {
                self.require_lead()?;
                self.check_revision(*expected_revision, self.project.revision)?;
                if reason.trim().is_empty() {
                    return Err(Refusal::InvalidShape {
                        field: "reason".into(),
                    });
                }
                let position = self
                    .high_water
                    .position
                    .successor()
                    .and_then(FeedPosition::successor)
                    .map_err(|_| Refusal::InvalidShape {
                        field: "position".into(),
                    })?;
                let close = FeedCursor {
                    epoch: self.admitted_epoch,
                    position,
                };
                let mut project = self.project.clone();
                project.closed_at = Some(close);
                project.revision = next_revision(project.revision)?;
                project.envelope = self.envelope(project.project_id.clone().into());
                Ok(OperationPlan::changed(vec![
                    Record::Project(project),
                    Record::Fact(self.lifecycle_fact(LifecycleFact::Closed {
                        position: close,
                        reason: reason.clone(),
                    })),
                ]))
            }
            BoardOperation::MoveRequest {
                move_id,
                move_name,
                evidence,
                reason,
                expected_revision,
                addressees,
            } => {
                self.require_new(move_id.clone().into())?;
                let definition = self.definitions.iter().find(|definition| {
                    self.project.pinned_definition.as_ref() == Some(&definition.reference)
                });
                let edge = definition.and_then(|definition| {
                    definition.edges.iter().find(|edge| {
                        edge.name == *move_name && Some(&edge.from) == self.project.status.as_ref()
                    })
                });
                let verdict = super::recorded_move_check::recorded_move_verdict(
                    super::recorded_move_check::RecordedMoveCheck {
                        project: &self.project,
                        definitions: &self.definitions,
                        records: &self.records,
                        actor: &self.request.actor,
                        move_name,
                        expected_revision: *expected_revision,
                        archived: self.archived,
                    },
                )?;
                let outcome = match &verdict {
                    MoveVerdict::Allowed {} => MoveOutcome::Accepted {},
                    MoveVerdict::Rejected { reasons } => MoveOutcome::Rejected {
                        reasons: reasons.clone(),
                    },
                };
                let movement = MoveRecord::new(MoveRecordProps {
                    envelope: self.envelope(move_id.clone().into()),
                    move_id: move_id.clone(),
                    project: self.request.project_id.clone(),
                    move_name: move_name.clone(),
                    endpoints: edge.map(|edge| MoveEndpoints {
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                    }),
                    evidence: evidence.clone(),
                    reason: reason.clone(),
                    requester: self.actor.clone(),
                    outcome,
                })
                .map_err(|_| Refusal::InvalidShape {
                    field: "endpoints".into(),
                })?;
                let mut plan = OperationPlan::changed(vec![Record::Move(movement)]);
                match verdict {
                    MoveVerdict::Allowed {} => {
                        let Some(edge) = edge else {
                            return Err(Refusal::Move(RejectReason::MoveNotAllowed {
                                edge: move_name.clone(),
                                required_role: None,
                            }));
                        };
                        let mut project = self.project.clone();
                        project.status = Some(edge.to.clone());
                        project.revision = next_revision(project.revision)?;
                        project.envelope = self.envelope(project.project_id.clone().into());
                        plan.records.push(Record::Project(project));
                        for addressee in addressees {
                            if !plan
                                .intents
                                .iter()
                                .any(|(_, existing)| existing == addressee)
                            {
                                plan.intents
                                    .push((move_id.clone().into(), addressee.clone()));
                            }
                        }
                    }
                    MoveVerdict::Rejected { reasons } => {
                        let repeated = self.records.iter().any(|record| matches!(record, Record::Move(previous) if previous.project == self.request.project_id && previous.move_name == *move_name && previous.requester.declared_identity().as_ref() == Some(&self.request.actor) && matches!(previous.outcome(), MoveOutcome::Rejected { reasons: original } if *original == reasons)));
                        plan.rejection = Some(reasons.into_iter().map(Refusal::Move).collect());
                        if !repeated {
                            plan.intents.push((
                                move_id.clone().into(),
                                NotifyTarget::Role(Assignment::Lead(
                                    self.request.project_id.clone(),
                                )),
                            ));
                        }
                    }
                }
                Ok(plan)
            }
            BoardOperation::FactRecord { fact } => {
                self.require_new(fact.fact_id.clone().into())?;
                if matches!(fact.kind, FactKind::OwnerWords | FactKind::Lifecycle(_)) {
                    return Err(Refusal::InvalidShape {
                        field: "kind".into(),
                    });
                }
                if !fact.links.iter().any(|link| {
                    matches!(
                        link.kind,
                        RecordKind::Project | RecordKind::Workstream | RecordKind::Task
                    )
                }) {
                    return Err(Refusal::InvalidShape {
                        field: "links".into(),
                    });
                }
                let late = if let Some(close) = self.project.closed_at {
                    if !fact
                        .anchor
                        .is_some_and(|anchor| anchor.position <= close.position)
                    {
                        return Err(Refusal::Closed {
                            subject: self.request.project_id.clone().into(),
                        });
                    }
                    true
                } else {
                    false
                };
                if let FactKind::EvaluatorResult { guard, .. } = &fact.kind
                    && ResourceName::try_from(guard.clone()).is_err()
                {
                    return Err(Refusal::InvalidShape {
                        field: "guard".into(),
                    });
                }
                Ok(OperationPlan::changed(vec![Record::Fact(Fact {
                    envelope: self.envelope(fact.fact_id.clone().into()),
                    fact_id: fact.fact_id.clone(),
                    kind: fact.kind.clone(),
                    links: fact.links.clone(),
                    producer_time: fact.producer_time,
                    anchor: fact.anchor,
                    late,
                })]))
            }
            _ => Err(Refusal::InvalidShape {
                field: "operation".into(),
            }),
        }
    }
}
