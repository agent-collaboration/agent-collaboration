//! Typed Workstream, Task, Trail and Role payloads retain their declared variants.
use crate::{
    fleet_reason_evidence::{EvidenceEntry, EvidenceRow},
    fleet_reason_fields::{EvidenceKind, ReasonFields, required},
    fleet_row_codec::corrupt,
};
use durable_steps::StoreError;
use message_board::{Refusal, RoleRefusal, TaskRefusal, TrailRefusal, WorkstreamRefusal};

pub(crate) fn encode(
    reason: &Refusal,
    evidence: &mut Vec<EvidenceEntry>,
) -> Result<ReasonFields, StoreError> {
    let mut fields = ReasonFields::default();
    match reason {
        Refusal::Workstream(reason) => {
            fields.group = "workstream".into();
            fields.kind = match reason {
                WorkstreamRefusal::RoleRequired { requirement } => {
                    fields.requirement = Some(requirement.clone());
                    "roleRequired"
                }
                WorkstreamRefusal::PairNotDistinct { holder } => {
                    fields.holder = Some(holder.clone());
                    "pairNotDistinct"
                }
                WorkstreamRefusal::Closed { workstream } => {
                    fields.workstream = Some(workstream.clone());
                    "closed"
                }
                WorkstreamRefusal::NotFound { record } => {
                    fields.record = Some(record.clone());
                    "notFound"
                }
            }
            .into();
        }
        Refusal::Task(reason) => {
            fields.group = "task".into();
            fields.kind = match reason {
                TaskRefusal::DependencyCycle { path } => {
                    fields.evidence = Some(crate::fleet_reason_evidence::header(
                        path,
                        EvidenceKind::Task,
                    )?);
                    evidence.extend(path.entries().iter().cloned().map(EvidenceEntry::Task));
                    "dependencyCycle"
                }
                TaskRefusal::DependencyMissing(task) => {
                    fields.task = Some(task.clone());
                    "dependencyMissing"
                }
                TaskRefusal::DependencyDropped(task) => {
                    fields.task = Some(task.clone());
                    "dependencyDropped"
                }
                TaskRefusal::DependencyCrossesProject(task) => {
                    fields.task = Some(task.clone());
                    "dependencyCrossesProject"
                }
                TaskRefusal::NotFound { record } => {
                    fields.record = Some(record.clone());
                    "notFound"
                }
                TaskRefusal::Closed { subject } => {
                    fields.record = Some(subject.clone());
                    "closed"
                }
                TaskRefusal::StaleRevision { current } => {
                    fields.revision = Some(*current);
                    "staleRevision"
                }
            }
            .into();
        }
        Refusal::Trail(reason) => {
            fields.group = "trail".into();
            fields.kind = match reason {
                TrailRefusal::SubjectRequired { assignments } => {
                    fields.evidence = Some(crate::fleet_reason_evidence::header(
                        assignments,
                        EvidenceKind::Assignment,
                    )?);
                    evidence.extend(
                        assignments
                            .entries()
                            .iter()
                            .cloned()
                            .map(EvidenceEntry::Assignment),
                    );
                    "subjectRequired"
                }
                TrailRefusal::NotFound { record } => {
                    fields.record = Some(record.clone());
                    "notFound"
                }
                TrailRefusal::InvalidShape { field } => {
                    fields.field = Some(field.clone());
                    "invalidShape"
                }
            }
            .into();
        }
        Refusal::Role(reason) => {
            fields.group = "role".into();
            fields.kind = match reason {
                RoleRefusal::NoHolder { subject, role } => {
                    fields.subject = Some(subject.clone());
                    fields.role = Some(*role);
                    "noHolder"
                }
                RoleRefusal::RoleRequired { requirement } => {
                    fields.requirement = Some(requirement.clone());
                    "roleRequired"
                }
                RoleRefusal::PairNotDistinct { holder } => {
                    fields.holder = Some(holder.clone());
                    "pairNotDistinct"
                }
                RoleRefusal::StandDownUnacknowledged {} => "standDownUnacknowledged",
            }
            .into();
        }
        _ => return Err(corrupt("reason.group", "not a grouped refusal")),
    }
    Ok(fields)
}

pub(crate) fn decode(
    mut fields: ReasonFields,
    evidence: Vec<EvidenceRow>,
    id: &str,
) -> Result<Refusal, StoreError> {
    Ok(match (fields.group.as_str(), fields.kind.as_str()) {
        ("workstream", "roleRequired") => {
            fields.active(&["requirement"])?;
            Refusal::Workstream(WorkstreamRefusal::RoleRequired {
                requirement: required(fields.requirement, "reason.requirement")?,
            })
        }
        ("workstream", "pairNotDistinct") => {
            fields.active(&["holder"])?;
            Refusal::Workstream(WorkstreamRefusal::PairNotDistinct {
                holder: required(fields.holder, "reason.holder")?,
            })
        }
        ("workstream", "closed") => {
            fields.active(&["workstream"])?;
            Refusal::Workstream(WorkstreamRefusal::Closed {
                workstream: required(fields.workstream, "reason.workstream")?,
            })
        }
        ("workstream", "notFound") => {
            fields.active(&["record"])?;
            Refusal::Workstream(WorkstreamRefusal::NotFound {
                record: required(fields.record, "reason.record")?,
            })
        }
        ("task", "dependencyCycle") => {
            fields.active(&["evidence"])?;
            Refusal::Task(TaskRefusal::DependencyCycle {
                path: crate::fleet_reason_evidence::tasks(
                    required(fields.evidence.take(), "reason.evidence")?,
                    evidence,
                    id,
                )?,
            })
        }
        ("task", "dependencyMissing") => {
            fields.active(&["task"])?;
            Refusal::Task(TaskRefusal::DependencyMissing(required(
                fields.task,
                "reason.task",
            )?))
        }
        ("task", "dependencyDropped") => {
            fields.active(&["task"])?;
            Refusal::Task(TaskRefusal::DependencyDropped(required(
                fields.task,
                "reason.task",
            )?))
        }
        ("task", "dependencyCrossesProject") => {
            fields.active(&["task"])?;
            Refusal::Task(TaskRefusal::DependencyCrossesProject(required(
                fields.task,
                "reason.task",
            )?))
        }
        ("task", "notFound") => {
            fields.active(&["record"])?;
            Refusal::Task(TaskRefusal::NotFound {
                record: required(fields.record, "reason.record")?,
            })
        }
        ("task", "closed") => {
            fields.active(&["record"])?;
            Refusal::Task(TaskRefusal::Closed {
                subject: required(fields.record, "reason.record")?,
            })
        }
        ("task", "staleRevision") => {
            fields.active(&["revision"])?;
            Refusal::Task(TaskRefusal::StaleRevision {
                current: required(fields.revision, "reason.revision")?,
            })
        }
        ("trail", "subjectRequired") => {
            fields.active(&["evidence"])?;
            Refusal::Trail(TrailRefusal::SubjectRequired {
                assignments: crate::fleet_reason_evidence::assignments(
                    required(fields.evidence.take(), "reason.evidence")?,
                    evidence,
                    id,
                )?,
            })
        }
        ("trail", "notFound") => {
            fields.active(&["record"])?;
            Refusal::Trail(TrailRefusal::NotFound {
                record: required(fields.record, "reason.record")?,
            })
        }
        ("trail", "invalidShape") => {
            fields.active(&["field"])?;
            Refusal::Trail(TrailRefusal::InvalidShape {
                field: required(fields.field, "reason.field")?,
            })
        }
        ("role", "noHolder") => {
            fields.active(&["subject", "role"])?;
            Refusal::Role(RoleRefusal::NoHolder {
                subject: required(fields.subject, "reason.subject")?,
                role: required(fields.role, "reason.role")?,
            })
        }
        ("role", "roleRequired") => {
            fields.active(&["requirement"])?;
            Refusal::Role(RoleRefusal::RoleRequired {
                requirement: required(fields.requirement, "reason.requirement")?,
            })
        }
        ("role", "pairNotDistinct") => {
            fields.active(&["holder"])?;
            Refusal::Role(RoleRefusal::PairNotDistinct {
                holder: required(fields.holder, "reason.holder")?,
            })
        }
        ("role", "standDownUnacknowledged") => {
            fields.active(&[])?;
            Refusal::Role(RoleRefusal::StandDownUnacknowledged {})
        }
        _ => return Err(corrupt("reason.kind", "unknown grouped refusal variant")),
    })
}
