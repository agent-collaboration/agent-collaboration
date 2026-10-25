use super::{
    operation_context::{OperationContext, OperationPlan},
    work_operations::next_revision,
};
use message_board::*;

impl OperationContext<'_> {
    pub(super) fn project_lifecycle_plan(&self) -> Result<OperationPlan, Refusal> {
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
