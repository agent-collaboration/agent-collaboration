use message_board::move_rules::{MoveCheck, check_move};
use message_board::*;
use sha2::{Digest, Sha256};
mod support;

#[allow(clippy::unwrap_used)]
fn name(value: &str) -> ResourceName {
    ResourceName::try_from(value.to_owned()).unwrap()
}
#[allow(clippy::unwrap_used)]
fn status(value: &str) -> StatusName {
    StatusName::try_from(value.to_owned()).unwrap()
}
#[allow(clippy::unwrap_used)]
fn actor(value: &str) -> Identity {
    Identity::Human {
        human_id: HumanId::try_from(value.to_owned()).unwrap(),
    }
}

#[allow(clippy::unwrap_used)]
fn fixture() -> (ProjectState, ProcessDefinition) {
    let project_id = ProjectId::generate();
    let source = "pure-rule fixture; no production statuses are selected";
    let reference = ProcessDefinitionRef {
        name: name("fixture"),
        version: name("1"),
        source_hash: CanonicalHash::try_from(format!("{:x}", Sha256::digest(source))).unwrap(),
    };
    let mut fleet_envelope = support::record_envelope(RecordId::generate(), project_id.clone());
    fleet_envelope.scope.project = None;
    fleet_envelope.scope.board = None;
    fleet_envelope.position = RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::ZERO,
        origin: support::actor_origin(),
    });
    let definition = ProcessDefinition {
        envelope: fleet_envelope,
        reference: reference.clone(),
        source: source.to_owned(),
        initial_status: status("started"),
        edges: vec![ProcessEdge {
            name: MoveName::try_from("finish".to_owned()).unwrap(),
            from: status("started"),
            to: status("finished"),
            required_role: AuthorityRole::Lead,
            guards: vec![GuardRequirement {
                name: name("verified"),
                evaluator: actor("verifier"),
            }],
        }],
    };
    let project = ProjectState {
        envelope: support::record_envelope(project_id.clone().into(), project_id.clone()),
        project_id,
        status: Some(status("started")),
        pinned_definition: Some(reference),
        revision: SubjectRevision::INITIAL,
        closed_at: None,
        description: Description::try_from(String::new()).unwrap(),
    };
    (project, definition)
}

#[test]
fn moves_require_the_pinned_edge_role_and_recorded_evaluator_result_without_writing() {
    let (project, definition) = fixture();
    let move_name = MoveName::try_from("finish".to_owned()).unwrap();
    for outcome in [
        None,
        Some(GuardOutcome::Met),
        Some(GuardOutcome::Unmet),
        Some(GuardOutcome::Unavailable),
    ] {
        for has_role in [false, true] {
            let before = project.clone();
            let roles = if has_role {
                vec![AuthorityRole::Lead]
            } else {
                vec![]
            };
            let results: Vec<RecordedGuardResult> = outcome
                .into_iter()
                .map(|outcome| RecordedGuardResult {
                    guard: name("verified"),
                    evaluator: actor("verifier"),
                    outcome,
                })
                .collect();
            let verdict = check_move(MoveCheck {
                project: &project,
                definition: Some(&definition),
                expected_revision: project.revision,
                move_name: &move_name,
                requester_roles: &roles,
                guard_results: &results,
                archived: false,
            });
            if has_role && outcome == Some(GuardOutcome::Met) {
                assert_eq!(verdict, MoveVerdict::Allowed {});
            } else {
                let MoveVerdict::Rejected { reasons } = verdict else {
                    panic!("missing preconditions passed")
                };
                assert_eq!(
                    reasons.iter().any(|reason| matches!(
                        reason,
                        RejectReason::MoveNotAllowed {
                            required_role: Some(AuthorityRole::Lead),
                            ..
                        }
                    )),
                    !has_role
                );
                assert_eq!(
                    reasons
                        .iter()
                        .any(|reason| matches!(reason, RejectReason::GuardUnavailable { .. })),
                    matches!(outcome, None | Some(GuardOutcome::Unavailable))
                );
                assert_eq!(
                    reasons
                        .iter()
                        .any(|reason| matches!(reason, RejectReason::GuardUnmet { .. })),
                    outcome == Some(GuardOutcome::Unmet)
                );
            }
            assert_eq!(project, before);
        }
    }
}

#[test]
fn stale_closed_archived_unpinned_and_wrong_evaluator_cannot_allow() {
    let (mut project, definition) = fixture();
    let move_name = MoveName::try_from("finish".to_owned()).unwrap();
    let mut results = vec![RecordedGuardResult {
        guard: name("verified"),
        evaluator: actor("other evaluator"),
        outcome: GuardOutcome::Met,
    }];
    let verdict = check_move(MoveCheck {
        project: &project,
        definition: Some(&definition),
        expected_revision: project.revision,
        move_name: &move_name,
        requester_roles: &[AuthorityRole::Lead],
        guard_results: &results,
        archived: false,
    });
    assert!(
        matches!(verdict, MoveVerdict::Rejected { reasons } if reasons.iter().any(|reason| matches!(reason, RejectReason::GuardUnavailable { .. })))
    );
    results.push(RecordedGuardResult {
        guard: name("verified"),
        evaluator: actor("verifier"),
        outcome: GuardOutcome::Met,
    });
    project.closed_at = Some(FeedCursor {
        epoch: EpochNumber::try_from(1).unwrap(),
        position: FeedPosition::try_from(10).unwrap(),
    });
    let verdict = check_move(MoveCheck {
        project: &project,
        definition: Some(&definition),
        expected_revision: SubjectRevision::try_from(1).unwrap(),
        move_name: &move_name,
        requester_roles: &[AuthorityRole::Lead],
        guard_results: &results,
        archived: true,
    });
    assert_eq!(
        verdict,
        MoveVerdict::Rejected {
            reasons: vec![
                RejectReason::Archived {},
                RejectReason::Closed {},
                RejectReason::StaleRevision {
                    current: SubjectRevision::INITIAL
                }
            ]
        }
    );
    project.closed_at = None;
    project.pinned_definition = None;
    assert!(
        matches!(check_move(MoveCheck { project: &project, definition: Some(&definition), expected_revision: project.revision,
        move_name: &move_name, requester_roles: &[AuthorityRole::Lead], guard_results: &results, archived: false }),
        MoveVerdict::Rejected { reasons } if reasons == vec![RejectReason::MoveNotAllowed { edge: move_name, required_role: None }])
    );
}
