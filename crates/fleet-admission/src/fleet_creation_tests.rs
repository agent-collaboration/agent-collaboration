//! A creation identity remains terminal after either first decision.
#![allow(clippy::panic_in_result_fn)]
use crate::*;
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[test]
fn another_creation_request_cannot_reuse_an_accepted_or_rejected_project_identity() -> TestResult {
    let project = ProjectId::generate();
    let request = crate::fleet_rule_tests::request(FleetOperation::ProjectDirectoryCreate {
        project: project.clone(),
        board: BoardId::generate(),
        title: ResourceName::try_from("fresh request for an old project identity".to_owned())?,
    })?;
    let origin = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440469".to_owned())?,
        machine: MachineLabel::try_from("verified creation carrier".to_owned())?,
    };
    let now = "2026-10-19T12:00:00Z".parse()?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    for decision in [
        StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed),
        StoredDecision::Rejected {
            reasons: vec![Refusal::Archived],
        },
    ] {
        let binding = ProjectCreationDecision {
            project: project.clone(),
            creating_request: RequestId::generate(),
            carrier_installation: origin.installation.clone(),
            decision,
        };
        let mut snapshot = FleetSnapshot::empty();
        snapshot.creation_decisions.push(binding.clone());

        let rejected = decide(&request, &snapshot, now, &ids, &origin)?;

        assert!(
            matches!(rejected.decision, RequestDecision::Rejected {ref reasons}
            if reasons == &vec![Refusal::AlreadyExists(project.clone().into())])
        );
        assert!(
            !rejected
                .writes
                .iter()
                .any(|write| matches!(write, FleetWrite::BindCreation { .. })),
            "a fresh request tried to replace the original binding"
        );
        assert_eq!(snapshot.creation_decisions, vec![binding]);
    }
    Ok(())
}

#[test]
fn the_first_rejected_creation_still_binds_its_request_and_verified_carrier() -> TestResult {
    let project = ProjectId::generate();
    let request = crate::fleet_rule_tests::request(FleetOperation::ProjectDirectoryCreate {
        project: project.clone(),
        board: BoardId::generate(),
        title: ResourceName::try_from("missing parents reject the first request".to_owned())?,
    })?;
    let carrier = ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440469".to_owned())?,
        machine: MachineLabel::try_from("actual carrier".to_owned())?,
    };
    let first = decide(
        &request,
        &FleetSnapshot::empty(),
        "2026-10-19T12:00:00Z".parse()?,
        &FleetIds {
            generated_record: RecordId::generate(),
        },
        &carrier,
    )?;

    assert!(matches!(first.decision, RequestDecision::Rejected { .. }));
    assert!(first.writes.iter().any(|write|matches!(write,FleetWrite::BindCreation {
        project:actual,creating_request,carrier_installation,
    } if actual==&project && creating_request==&request.request_id && carrier_installation==&carrier.installation)));
    assert!(reads_for(&request).keys.iter().any(
        |key| matches!(key,FleetReadKey::CreationDecision {project:actual} if actual==&project)
    ));
    Ok(())
}
