//! The read-only check and admission resolve the same recorded roles and guard facts.
use message_board::*;
pub(super) struct RecordedMoveCheck<'state> {
    pub project: &'state ProjectState,
    pub definitions: &'state [ProcessDefinition],
    pub records: &'state [Record],
    pub actor: &'state Identity,
    pub move_name: &'state MoveName,
    pub expected_revision: SubjectRevision,
    pub archived: bool,
}
pub(super) fn recorded_move_verdict(input: RecordedMoveCheck<'_>) -> Result<MoveVerdict, Refusal> {
    let definition = input
        .definitions
        .iter()
        .find(|definition| input.project.pinned_definition.as_ref() == Some(&definition.reference));
    let roles: Vec<_> = input
        .records
        .iter()
        .filter_map(|record| match record {
            Record::RoleAssignment(assignment)
                if assignment.subject()
                    == RoleSubject::Project(input.project.project_id.clone())
                    && assignment.role() == AuthorityRole::Lead
                    && assignment.ended.is_none()
                    && assignment.holder.declared_identity() == Some(input.actor) =>
            {
                Some(AuthorityRole::Lead)
            }
            _ => None,
        })
        .collect();
    let mut facts: Vec<_> = input
        .records
        .iter()
        .filter_map(|record| match record {
            Record::Fact(fact) => Some(fact),
            _ => None,
        })
        .collect();
    facts.sort_by_key(|fact| match fact.envelope.position {
        RecordPosition::Project(cursor) => cursor.position,
        RecordPosition::Fleet(_) | RecordPosition::Unsequenced => FeedPosition::ZERO,
    });
    let results: Vec<_> = facts
        .iter()
        .filter_map(|fact| match &fact.kind {
            FactKind::EvaluatorResult {
                guard,
                subject,
                outcome,
                evaluator,
                ..
            } if *subject == RecordRef::from(input.project.project_id.clone()) => {
                Some((guard, evaluator, outcome))
            }
            _ => None,
        })
        .map(|(guard, evaluator, outcome)| {
            Ok(RecordedGuardResult {
                guard: ResourceName::try_from(guard.clone()).map_err(|_| {
                    Refusal::InvalidShape {
                        field: "guard".to_owned(),
                    }
                })?,
                evaluator: evaluator
                    .actor
                    .declared_identity()
                    .ok_or_else(|| Refusal::InvalidShape {
                        field: "evaluator".to_owned(),
                    })?
                    .clone(),
                outcome: *outcome,
            })
        })
        .collect::<Result<_, Refusal>>()?;
    Ok(move_rules::check_move(move_rules::MoveCheck {
        project: input.project,
        definition,
        expected_revision: input.expected_revision,
        move_name: input.move_name,
        requester_roles: &roles,
        guard_results: &results,
        archived: input.archived,
    }))
}
