//! Reserved tags derive pending work; settlement authority comes from immutable role history.
use super::ViewSnapshot;
use message_board::*;

use message_board_storage::{PendingTrailInputs, TrailActivityMetadata};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct PendingWork {
    pub handbacks: Vec<TrailActivityMetadata>,
    pub questions: Vec<TrailActivityMetadata>,
}
/// Settlement remains a board-view rule. Native reads supply only its typed operands.
pub(super) fn pending_work(inputs: PendingTrailInputs) -> Result<PendingWork, BoardError> {
    struct TrailSettlement {
        decision: FeedPosition,
        lead_decision: Option<FeedPosition>,
    }
    let mut settlements = BTreeMap::<WorkTrailRowId, TrailSettlement>::new();
    for decision in inputs.decisions {
        let position = row_position(&decision.row)?.position;
        let lead = decision.authority.is_some_and(|authority| {
            authority.ended.is_none() && decision.row.declared_actor() == Some(&authority.holder)
        });
        for link in decision.row.links {
            let EvidencePointer::Record { record } = link else {
                continue;
            };
            if record.kind != RecordKind::WorkTrailRow {
                continue;
            }
            let id = WorkTrailRowId::try_from(record.id.as_str().to_owned())
                .map_err(|_| BoardError::board_unavailable())?;
            let settlement = settlements.entry(id).or_insert(TrailSettlement {
                decision: position,
                lead_decision: None,
            });
            settlement.decision = settlement.decision.max(position);
            if lead {
                settlement.lead_decision = Some(
                    settlement
                        .lead_decision
                        .map_or(position, |previous| previous.max(position)),
                );
            }
        }
    }
    let mut handbacks = Vec::new();
    let mut questions = Vec::new();
    for row in inputs.candidates {
        let position = row_position(&row)?.position;
        let settlement = settlements.get(&row.row_id);
        if row.has_tag("handback")
            && !settlement.is_some_and(|item| {
                item.lead_decision
                    .is_some_and(|decision| decision > position)
            })
        {
            handbacks.push((position, row.clone()));
        }
        if row.has_tag("owner-question") && !settlement.is_some_and(|item| item.decision > position)
        {
            questions.push((position, row));
        }
    }
    handbacks.sort_by_key(|(position, _)| *position);
    questions.sort_by_key(|(position, _)| *position);
    Ok(PendingWork {
        handbacks: handbacks.into_iter().map(|(_, row)| row).collect(),
        questions: questions.into_iter().map(|(_, row)| row).collect(),
    })
}

pub(crate) fn lead_owed(
    snapshot: &ViewSnapshot,
) -> Result<Vec<(FeedPosition, LeadOwed)>, BoardError> {
    let mut owed: Vec<_> = snapshot
        .pending
        .handbacks
        .iter()
        .map(|item| {
            Ok((
                row_position(item)?.position,
                LeadOwed {
                    project: snapshot.project.clone(),
                    subject: item.row_id.clone().into(),
                    obligation: LeadObligation::Assess,
                },
            ))
        })
        .collect::<Result<Vec<_>, BoardError>>()?;
    let tasks = snapshot.tasks();
    for record in snapshot.records.values() {
        if let Record::Workstream(row) = record
            && matches!(row.state, WorkstreamState::Open {})
        {
            let in_workstream: Vec<_> = tasks
                .iter()
                .filter(|task| task.workstream_id == row.workstream_id)
                .collect();
            if !in_workstream.is_empty()
                && in_workstream
                    .iter()
                    .all(|task| matches!(task.status, TaskStatus::Done | TaskStatus::Dropped))
            {
                let mut positions = Vec::new();
                for task in in_workstream {
                    positions.push(
                        snapshot
                            .position(&task.task_id.clone().into())
                            .ok_or_else(BoardError::board_unavailable)?
                            .position,
                    );
                }
                let position = positions
                    .into_iter()
                    .max()
                    .ok_or_else(BoardError::board_unavailable)?;
                owed.push((
                    position,
                    LeadOwed {
                        project: snapshot.project.clone(),
                        subject: row.workstream_id.clone().into(),
                        obligation: LeadObligation::CloseOut,
                    },
                ));
            }
        }
    }
    for rejection in super::work_view(snapshot, false)?.rejected_since_last_move {
        let cursor = snapshot
            .position(&rejection.move_id.clone().into())
            .ok_or_else(BoardError::board_unavailable)?;
        owed.push((
            cursor.position,
            LeadOwed {
                project: snapshot.project.clone(),
                subject: rejection.move_id.into(),
                obligation: LeadObligation::AnswerRejection,
            },
        ));
    }
    owed.sort_by_key(|(position, _)| *position);
    Ok(owed)
}
pub(crate) fn owner_waiting(snapshot: &ViewSnapshot) -> Result<Vec<OwnerWait>, BoardError> {
    snapshot
        .pending
        .questions
        .iter()
        .map(|item| {
            Ok(OwnerWait {
                project: snapshot.project.clone(),
                subject: item.row_id.clone().into(),
                blocks: item
                    .links
                    .iter()
                    .filter_map(|link| match link {
                        EvidencePointer::Record { record } => Some(record.clone()),
                        _ => None,
                    })
                    .collect(),
                since: row_position(item)?,
            })
        })
        .collect::<Result<Vec<_>, BoardError>>()
}
pub(super) fn row_position(
    row: &message_board_storage::TrailActivityMetadata,
) -> Result<FeedCursor, BoardError> {
    row.position().map_err(|_| BoardError::board_unavailable())
}
