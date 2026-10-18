use super::operation_context::{OperationContext, OperationPlan};
use message_board::*;

impl OperationContext<'_> {
    pub(super) fn work_plan(&self) -> Result<OperationPlan, Refusal> {
        match &self.request.operation {
            BoardOperation::WorkstreamCreate {
                workstream_id,
                name,
                goal,
            } => {
                self.require_lead()?;
                self.require_new(workstream_id.clone().into())?;
                Ok(OperationPlan::changed(vec![Record::Workstream(
                    Workstream {
                        envelope: self.envelope(workstream_id.clone().into()),
                        workstream_id: workstream_id.clone(),
                        project_id: self.request.project_id.clone(),
                        name: name.clone(),
                        goal: goal.clone(),
                        state: WorkstreamState::Open {},
                        revision: SubjectRevision::INITIAL,
                    },
                )]))
            }
            BoardOperation::WorkstreamRename {
                workstream_id,
                name,
                expected_revision,
            } => {
                self.require_lead()?;
                let mut row = self.require_open_workstream(workstream_id)?;
                self.check_revision(*expected_revision, row.revision)?;
                if row.name == *name {
                    return Ok(OperationPlan::unchanged());
                }
                row.name = name.clone();
                row.revision = next_revision(row.revision)?;
                row.envelope = self.envelope(row.workstream_id.clone().into());
                Ok(OperationPlan::changed(vec![Record::Workstream(row)]))
            }
            BoardOperation::WorkstreamClose {
                workstream_id,
                reason,
                note,
                expected_revision,
            } => {
                self.require_lead()?;
                let mut row = self.workstream(workstream_id)?;
                self.check_revision(*expected_revision, row.revision)?;
                if !matches!(row.state, WorkstreamState::Open {}) {
                    return Err(Refusal::Workstream(WorkstreamRefusal::Closed {
                        workstream: workstream_id.clone(),
                    }));
                }
                row.state = WorkstreamState::Closed {
                    reason: *reason,
                    note: note.clone(),
                };
                row.revision = next_revision(row.revision)?;
                row.envelope = self.envelope(workstream_id.clone().into());
                let fact = self.lifecycle_fact(LifecycleFact::WorkstreamClosed {
                    workstream: workstream_id.clone(),
                    reason: *reason,
                });
                Ok(OperationPlan::changed(vec![
                    Record::Workstream(row),
                    Record::Fact(fact),
                ]))
            }
            BoardOperation::WorkTrailAdd {
                row_id,
                subject,
                text,
                tags,
                links,
            } => {
                self.require_new(row_id.clone().into())?;
                let assignments: Vec<_> = self
                    .records
                    .iter()
                    .filter_map(|record| match record {
                        Record::RoleAssignment(row)
                            if row.ended.is_none()
                                && row.holder.declared_identity() == Some(&self.request.actor) =>
                        {
                            Some(row.assignment())
                        }
                        _ => None,
                    })
                    .collect();
                let subject = work_rules::resolve_trail_subject(subject.clone(), &assignments)
                    .map_err(Refusal::Trail)?;
                match &subject {
                    TrailSubject::Project => {}
                    TrailSubject::Workstream(id) => {
                        self.workstream(id)?;
                    }
                    TrailSubject::Task(id) => {
                        self.task(id)?;
                    }
                }
                if tags.iter().any(|tag| tag.as_str() == "handback") {
                    let workstream = match &subject {
                        TrailSubject::Workstream(id) => Some(id.clone()),
                        TrailSubject::Task(id) => Some(self.task(id)?.workstream_id),
                        TrailSubject::Project => None,
                    };
                    let holds_pair_role = workstream.is_some_and(|workstream| {
                        self.records.iter().any(|record| matches!(record,
                            Record::RoleAssignment(role)
                            if role.ended.is_none()
                                && role.subject() == RoleSubject::Workstream(workstream.clone())
                                && matches!(role.role(), AuthorityRole::Implementer | AuthorityRole::Verifier)
                                && role.holder.declared_identity() == Some(&self.request.actor)
                        ))
                    });
                    if !holds_pair_role {
                        // Name one eligible role using the existing refusal shape; either pair role admits.
                        return Err(Refusal::Role(RoleRefusal::RoleRequired {
                            requirement: RoleRequirement::Holds(AuthorityRole::Implementer),
                        }));
                    }
                }
                if tags.iter().any(|tag| tag.as_str() == "owner-question")
                    && !links
                        .iter()
                        .any(|link| matches!(link, EvidencePointer::Record { .. }))
                {
                    return Err(Refusal::InvalidShape {
                        field: "links".to_owned(),
                    });
                }
                Ok(OperationPlan::changed(vec![Record::WorkTrailRow(
                    WorkTrailRow {
                        envelope: self.envelope(row_id.clone().into()),
                        row_id: row_id.clone(),
                        project_id: self.request.project_id.clone(),
                        subject,
                        text: text.clone(),
                        tags: tags.clone(),
                        links: links.clone(),
                        redacted: false,
                    },
                )]))
            }
            BoardOperation::WorkTrailRedact { row_id, redacted } => {
                let Record::WorkTrailRow(row) = self.record(&row_id.clone().into())? else {
                    return Err(Refusal::Trail(TrailRefusal::NotFound {
                        record: row_id.clone().into(),
                    }));
                };
                if row.redacted == *redacted {
                    return Ok(OperationPlan::unchanged());
                }
                Ok(OperationPlan::changed(vec![Record::TrailRedaction(
                    TrailRedaction {
                        envelope: self.envelope(RecordId::generate()),
                        row: row_id.clone(),
                        redacted: *redacted,
                    },
                )]))
            }
            _ => Err(Refusal::InvalidShape {
                field: "operation".into(),
            }),
        }
    }
    pub(super) fn lifecycle_fact(&self, kind: LifecycleFact) -> Fact {
        let fact_id = FactId::generate();
        Fact {
            envelope: self.envelope(fact_id.clone().into()),
            fact_id,
            kind: FactKind::Lifecycle(kind),
            links: Vec::new(),
            producer_time: self.now,
            anchor: Some(self.high_water),
            late: false,
        }
    }
}

pub(super) fn next_revision(current: SubjectRevision) -> Result<SubjectRevision, Refusal> {
    current.successor().map_err(|_| Refusal::InvalidShape {
        field: "revision".into(),
    })
}
