//! Check recorded project state and evaluator results without changing either.

use crate::{
    AuthorityRole, GuardOutcome, MoveName, MoveVerdict, ProcessDefinition, ProjectState,
    RecordedGuardResult, RejectReason, SubjectRevision,
};

pub struct MoveCheck<'state> {
    pub project: &'state ProjectState,
    pub definition: Option<&'state ProcessDefinition>,
    pub expected_revision: SubjectRevision,
    pub move_name: &'state MoveName,
    pub requester_roles: &'state [AuthorityRole],
    pub guard_results: &'state [RecordedGuardResult],
    pub archived: bool,
}

#[must_use]
pub fn check_move(input: MoveCheck<'_>) -> MoveVerdict {
    let mut reasons = Vec::new();
    if input.archived {
        reasons.push(RejectReason::Archived {});
    }
    if input.project.closed_at.is_some() {
        reasons.push(RejectReason::Closed {});
    }
    if input.project.revision != input.expected_revision {
        reasons.push(RejectReason::StaleRevision {
            current: input.project.revision,
        });
    }
    let edge = input
        .definition
        .filter(|definition| {
            input.project.pinned_definition.as_ref() == Some(&definition.reference)
        })
        .and_then(|definition| {
            definition.edges.iter().find(|edge| {
                edge.name == *input.move_name && input.project.status.as_ref() == Some(&edge.from)
            })
        });
    if let Some(edge) = edge {
        if !input.requester_roles.contains(&edge.required_role) {
            reasons.push(RejectReason::MoveNotAllowed {
                edge: input.move_name.clone(),
                required_role: Some(edge.required_role),
            });
        }
        for guard in &edge.guards {
            let outcome = input
                .guard_results
                .iter()
                .rev()
                .find(|result| result.guard == guard.name && result.evaluator == guard.evaluator)
                .map(|result| result.outcome);
            match outcome {
                Some(GuardOutcome::Met) => {}
                Some(GuardOutcome::Unmet) => reasons.push(RejectReason::GuardUnmet {
                    guard: guard.name.as_str().to_owned(),
                    evaluator: guard.evaluator.clone(),
                }),
                Some(GuardOutcome::Unavailable) | None => {
                    reasons.push(RejectReason::GuardUnavailable {
                        guard: guard.name.as_str().to_owned(),
                        evaluator: guard.evaluator.clone(),
                    })
                }
            }
        }
    } else {
        reasons.push(RejectReason::MoveNotAllowed {
            edge: input.move_name.clone(),
            required_role: None,
        });
    }
    if reasons.is_empty() {
        MoveVerdict::Allowed {}
    } else {
        MoveVerdict::Rejected { reasons }
    }
}
