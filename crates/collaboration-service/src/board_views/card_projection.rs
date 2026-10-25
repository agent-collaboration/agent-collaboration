//! Cards select actor authority and subject trail without inventing duties or evidence.
use super::{ViewSnapshot, pending_projection::lead_owed};
use message_board::*;

const RECENT_TRAIL_LIMIT: usize = 20;
const SILENCE_RULE: &str = "A claim the board does not contradict is unrecorded, not false.";

pub(crate) fn continuation_card(
    mut snapshot: ViewSnapshot,
    request: &ContinuationCardRequest,
    session_state: SessionRunningState,
) -> Result<ContinuationCard, BoardError> {
    let subject = subject_reference(&snapshot, &request.subject);
    let record = snapshot
        .records
        .get(&(subject.kind, subject.id.clone()))
        .ok_or_else(|| BoardError::invalid_field("subject", "was not found in this project"))?;
    let workstream = match record {
        Record::Workstream(row) => Some(row.workstream_id.clone()),
        Record::Task(row) => Some(row.workstream_id.clone()),
        _ => None,
    };
    let assignment = snapshot
        .roles()
        .into_iter()
        .filter(|role| {
            role.holder.declared_identity().as_ref() == Some(&request.actor)
                && (role.role() == AuthorityRole::Lead
                    || workstream
                        .as_ref()
                        .is_some_and(|id| role.subject() == RoleSubject::Workstream(id.clone())))
        })
        // Lead duties still apply when the same actor also holds a workstream role.
        .min_by_key(|role| role.role() != AuthorityRole::Lead)
        .map(RoleAssignment::assignment);
    let is_lead = matches!(assignment, Some(Assignment::Lead(_)));
    let is_pair = matches!(
        assignment,
        Some(Assignment::Implementer(_) | Assignment::Verifier(_))
    );
    let pending_handbacks = snapshot
        .pending
        .handbacks
        .iter()
        .filter(|row| {
            matches_subject(&snapshot, &row.subject, &request.subject)
                && (is_lead || is_pair && row.declared_actor().as_ref() == Some(&request.actor))
        })
        .map(|row| row.row_id.clone().into())
        .collect();
    let pending_decisions = snapshot
        .pending
        .questions
        .iter()
        .filter(|row| matches_subject(&snapshot, &row.subject, &request.subject))
        .map(|row| row.row_id.clone().into())
        .collect();
    let next_obligation = if is_lead {
        lead_owed(&snapshot)?
            .into_iter()
            .next()
            .map(|(_, item)| item)
    } else {
        None
    };
    let plan_revision = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::PlanRevision(row) if row.superseded_by.is_none() => Some(row),
            _ => None,
        })
        .max_by_key(|row| {
            snapshot
                .position(&RecordRef {
                    kind: RecordKind::PlanRevision,
                    id: row.envelope.id.clone(),
                })
                .map(|cursor| cursor.position)
        })
        .map(|row| row.hash.clone());
    let view = super::work_view(&snapshot, false)?;
    let blockers = view
        .blocked
        .into_iter()
        .filter(|item| match &request.subject {
            TrailSubject::Project => true,
            TrailSubject::Task(id) => &item.task == id,
            TrailSubject::Workstream(id) => snapshot
                .tasks()
                .iter()
                .any(|task| task.task_id == item.task && &task.workstream_id == id),
        })
        .collect();
    let notify_intents = snapshot
        .records
        .values()
        .filter_map(|record| match record {
            Record::NotifyIntent(intent) => Some(intent.clone()),
            _ => None,
        })
        .collect();
    let mut card = ContinuationCard {
        subject,
        plan_revision,
        assignment,
        pending_handbacks,
        pending_decisions,
        next_obligation,
        recent_trail: Vec::new(),
        counts: snapshot.counts.clone(),
        as_of: snapshot.as_of,
        replicated_through: snapshot.replicated_through,
        source: snapshot.source,
        silence_rule: SILENCE_RULE.to_owned(),
        notify_intents,
        blockers,
        session_state,
    };
    // Keep a newest-first prefix under the existing 900 KiB record budget. Full counts and
    // pending references still cover the snapshot; the paged trail serves older content.
    let budget = crate::collaboration_application::ResultByteBudget::new(900 * 1024, 0);
    if !budget.admits(&card) {
        return Err(BoardError::board_unavailable());
    }
    for stored in std::mem::take(&mut snapshot.recent_trail)
        .into_iter()
        .take(RECENT_TRAIL_LIMIT)
    {
        let Record::WorkTrailRow(row) = stored.decode().map_err(|error| {
            tracing::warn!(%error,"recent trail unavailable");
            BoardError::board_unavailable()
        })?
        else {
            return Err(BoardError::board_unavailable());
        };
        if row.redacted || !matches_subject(&snapshot, &row.subject, &request.subject) {
            return Err(BoardError::board_unavailable());
        }
        card.recent_trail.push(row);
        if !budget.admits(&card) {
            card.recent_trail.pop();
            break;
        }
    }
    Ok(card)
}

fn subject_reference(snapshot: &ViewSnapshot, subject: &TrailSubject) -> RecordRef {
    match subject {
        TrailSubject::Project => snapshot.project.clone().into(),
        TrailSubject::Workstream(id) => id.clone().into(),
        TrailSubject::Task(id) => id.clone().into(),
    }
}
fn matches_subject(snapshot: &ViewSnapshot, row: &TrailSubject, selection: &TrailSubject) -> bool {
    match selection {
        TrailSubject::Project => true,
        TrailSubject::Task(_) => row == selection,
        TrailSubject::Workstream(id) => {
            row == selection
                || matches!(row,TrailSubject::Task(task) if snapshot.tasks().iter().any(|item|item.task_id==*task && item.workstream_id==*id))
        }
    }
}
