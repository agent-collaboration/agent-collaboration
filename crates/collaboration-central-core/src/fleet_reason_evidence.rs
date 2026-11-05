//! Ordered refusal evidence retains the original complete commitment, including abridged prefixes.
use crate::{
    fleet_column_codec::{canonical, optional_text, text, unsigned},
    fleet_reason_fields::{EvidenceHeader, EvidenceKind},
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_fleet_reasons::StoredReasonEvidence,
};
use durable_steps::StoreError;
use message_board::{AbridgedEvidence, Assignment, TaskId};

pub(crate) enum EvidenceEntry {
    Task(TaskId),
    Assignment(Assignment),
}
pub(crate) struct EvidenceRow {
    pub reason: String,
    pub ordinal: u64,
    pub entry: EvidenceEntry,
}
pub(crate) fn decode(row: StoredReasonEvidence) -> Result<EvidenceRow, StoreError> {
    check_private_id(
        &text(row.row_id, row.row_id_class, "reason.evidence.id")?,
        "reason.evidence.id",
    )?;
    let reason = text(row.reason_id, row.reason_id_class, "reason.evidence.reason")?;
    check_private_id(&reason, "reason.evidence.reason")?;
    let ordinal = unsigned(row.ordinal, row.ordinal_class, "reason.evidence.ordinal")?;
    check_millis(
        row.created_at,
        row.created_at_class,
        "reason.evidence.created_at",
    )?;
    check_millis(
        row.updated_at,
        row.updated_at_class,
        "reason.evidence.updated_at",
    )?;
    let entry = match (
        text(row.entry_kind, row.entry_kind_class, "reason.evidence.kind")?.as_str(),
        optional_text(row.task_id, row.task_id_class, "reason.evidence.task")?,
        optional_text(
            row.assignment_kind,
            row.assignment_kind_class,
            "reason.evidence.assignment",
        )?,
        optional_text(
            row.assignment_project_id,
            row.assignment_project_id_class,
            "reason.evidence.project",
        )?,
        optional_text(
            row.assignment_workstream_id,
            row.assignment_workstream_id_class,
            "reason.evidence.workstream",
        )?,
    ) {
        ("task", Some(task), None, None, None) => {
            EvidenceEntry::Task(canonical(task, "reason.evidence.task")?)
        }
        ("assignment", None, Some(kind), project, workstream) => {
            EvidenceEntry::Assignment(match (kind.as_str(), project, workstream) {
                ("lead", Some(project), None) => {
                    Assignment::Lead(canonical(project, "reason.evidence.project")?)
                }
                ("advisor", Some(project), None) => {
                    Assignment::Advisor(canonical(project, "reason.evidence.project")?)
                }
                ("implementer", None, Some(workstream)) => {
                    Assignment::Implementer(canonical(workstream, "reason.evidence.workstream")?)
                }
                ("verifier", None, Some(workstream)) => {
                    Assignment::Verifier(canonical(workstream, "reason.evidence.workstream")?)
                }
                ("reviewer", None, Some(workstream)) => {
                    Assignment::Reviewer(canonical(workstream, "reason.evidence.workstream")?)
                }
                ("operator", None, Some(workstream)) => {
                    Assignment::Operator(canonical(workstream, "reason.evidence.workstream")?)
                }
                _ => {
                    return Err(corrupt(
                        "reason.evidence.assignment",
                        "unknown assignment kind or inactive subject fields",
                    ));
                }
            })
        }
        _ => {
            return Err(corrupt(
                "reason.evidence.entry",
                "unknown entry kind or inactive fields",
            ));
        }
    };
    Ok(EvidenceRow {
        reason,
        ordinal,
        entry,
    })
}

fn check_rows(
    header: &EvidenceHeader,
    rows: &[EvidenceRow],
    reason: &str,
) -> Result<(), StoreError> {
    if header.returned > 256 || usize::try_from(header.returned).ok() != Some(rows.len()) {
        return Err(corrupt(
            "reason.evidence.count",
            "rows differ from the bounded returned count",
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        if row.reason != reason || usize::try_from(row.ordinal).ok() != Some(index) {
            return Err(corrupt(
                "reason.evidence.ordinal",
                "wrong parent or noncontiguous evidence",
            ));
        }
    }
    Ok(())
}
pub(crate) fn tasks(
    header: EvidenceHeader,
    rows: Vec<EvidenceRow>,
    reason: &str,
) -> Result<AbridgedEvidence<TaskId>, StoreError> {
    check_rows(&header, &rows, reason)?;
    if header.kind != EvidenceKind::Task {
        return Err(corrupt(
            "reason.evidence.kind",
            "task refusal has another evidence kind",
        ));
    }
    let entries = rows
        .into_iter()
        .map(|row| match row.entry {
            EvidenceEntry::Task(task) => Ok(task),
            EvidenceEntry::Assignment(_) => Err(corrupt(
                "reason.evidence.kind",
                "task refusal has assignment evidence",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    AbridgedEvidence::from_parts(entries, header.total, header.digest, header.abridged)
        .map_err(|error| corrupt("reason.evidence", error))
}
pub(crate) fn assignments(
    header: EvidenceHeader,
    rows: Vec<EvidenceRow>,
    reason: &str,
) -> Result<AbridgedEvidence<Assignment>, StoreError> {
    check_rows(&header, &rows, reason)?;
    if header.kind != EvidenceKind::Assignment {
        return Err(corrupt(
            "reason.evidence.kind",
            "assignment refusal has another evidence kind",
        ));
    }
    let entries = rows
        .into_iter()
        .map(|row| match row.entry {
            EvidenceEntry::Assignment(assignment) => Ok(assignment),
            EvidenceEntry::Task(_) => Err(corrupt(
                "reason.evidence.kind",
                "assignment refusal has task evidence",
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    AbridgedEvidence::from_parts(entries, header.total, header.digest, header.abridged)
        .map_err(|error| corrupt("reason.evidence", error))
}

pub(crate) fn header<T: message_board::EvidenceEntry>(
    evidence: &message_board::AbridgedEvidence<T>,
    kind: EvidenceKind,
) -> Result<EvidenceHeader, StoreError> {
    Ok(EvidenceHeader {
        kind,
        returned: evidence
            .entries()
            .len()
            .try_into()
            .map_err(StoreError::backend)?,
        total: evidence.total_count(),
        digest: evidence.digest().clone(),
        abridged: evidence.is_abridged(),
    })
}
