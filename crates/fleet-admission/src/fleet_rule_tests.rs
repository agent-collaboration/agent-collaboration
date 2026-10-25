#![allow(clippy::panic_in_result_fn)]
use crate::*;
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn origin() -> Result<ActorOrigin, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("fixture machine".to_owned())?,
    })
}

pub(crate) fn request(
    operation: FleetOperation,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: WorkspaceId::try_from("01890f2e-7b4c-7cc0-98c4-000000000100".to_owned())?,
        actor: RecordedActor::declared(Identity::Human {
            human_id: HumanId::try_from("owner".to_owned())?,
        }),
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}

#[test]
fn canonical_content_distinguishes_full_session_refs_without_recording_context() -> TestResult {
    // Arrange: the same request names a session on its home Router.
    let mut first = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("identity proof".to_owned())?,
        description: Description::try_from("same operation".to_owned())?,
    })?;
    let session: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"fixture"},
        "sessionId":"fixture-session"}))?;
    first.actor = RecordedActor::Session {
        session: session.clone(),
    };
    let mut second = first.clone();

    // Act: copying the declared ref preserves its key; changing its home changes it.
    second.actor = RecordedActor::declared(Identity::Session {
        session: session.clone(),
    });
    assert_eq!(canonical_hash(&first)?, canonical_hash(&second)?);
    let mut other_home = session;
    other_home.endpoint.service_id =
        ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?;
    second.actor = RecordedActor::Session {
        session: other_home,
    };

    // Assert: home installation is already part of the ref.
    assert_ne!(canonical_hash(&first)?, canonical_hash(&second)?);
    Ok(())
}

#[test]
fn human_fleet_records_keep_the_admitted_origin_across_replay_from_another_machine() -> TestResult {
    let request = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("origin proof".to_owned())?,
        description: Description::try_from("declared human on an enrolled machine".to_owned())?,
    })?;
    let now = "2026-10-12T00:00:00Z".parse()?;
    let first = decide(&request, &FleetSnapshot::empty(), now, &origin()?)?;
    assert_eq!(first.feed.len(), 2);
    for entry in &first.feed {
        assert_eq!(entry.record.envelope().position.origin(), Some(&origin()?));
        assert_eq!(
            entry.record.envelope().position.fleet_position(),
            Some(entry.position)
        );
        assert_eq!(entry.record.envelope().actor, request.actor);
    }
    let mut snapshot = FleetSnapshot::empty();
    snapshot.ledger = first.writes.iter().find_map(|write| match write {
        FleetWrite::Ledger(ledger) => Some(ledger.as_ref().clone()),
        _ => None,
    });
    snapshot.decision_receipts = first
        .writes
        .iter()
        .filter_map(|write| match write {
            FleetWrite::DecisionReceipt { effect, .. } => Some(effect.clone()),
            _ => None,
        })
        .collect();
    let another_origin = ActorOrigin {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?,
        machine: MachineLabel::try_from("another enrolled machine".to_owned())?,
    };
    let replay = decide(&request, &snapshot, now, &another_origin)?;
    assert_eq!(replay.decision, first.decision);
    assert!(replay.feed.is_empty() && replay.writes.is_empty());
    Ok(())
}

#[test]
fn declared_sessions_keep_their_home_while_only_router_actors_bind_to_record_origin() -> TestResult
{
    // Arrange: one declared session and one internal Router actor share a home.
    let mut request = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("actor origin proof".to_owned())?,
        description: Description::try_from(
            "declared attribution and verified provenance".to_owned(),
        )?,
    })?;
    let session = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"fixture"},
        "sessionId":"fixture-session"
    }))?;
    for actor in [
        RecordedActor::Session { session },
        RecordedActor::Router {
            component: RouterComponent::SubscriptionDelivery,
            installation: origin()?.installation,
        },
    ] {
        request.actor = actor;
        for admitted in [
            origin()?,
            ActorOrigin {
                machine: MachineLabel::try_from("changed label".to_owned())?,
                ..origin()?
            },
            ActorOrigin {
                installation: ServiceId::try_from(
                    "00000000-0000-4000-8000-000000000002".to_owned(),
                )?,
                ..origin()?
            },
        ] {
            // Act: admission receives provenance separately from declared identity.
            let decision = decide(
                &request,
                &FleetSnapshot::empty(),
                "2026-10-12T00:00:00Z".parse()?,
                &admitted,
            );

            // Assert: only the internal Router variant makes an installation claim.
            if matches!(&request.actor, RecordedActor::Router { .. })
                && admitted.installation != origin()?.installation
            {
                assert!(matches!(
                    decision,
                    Err(FleetAdmissionError::InvalidActorOrigin)
                ));
            } else {
                let decision = decision?;
                assert_eq!(decision.feed.len(), 2);
                for entry in decision.feed {
                    assert_eq!(entry.record.envelope().actor, request.actor);
                    assert_eq!(entry.record.envelope().position.origin(), Some(&admitted));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn create_replay_conflict_and_name_collision_are_decision_table_cases() -> TestResult {
    let req = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Router".to_owned())?,
        description: Description::try_from("Board work".to_owned())?,
    })?;
    let now = "2026-10-12T00:00:00Z".parse()?;
    let initial = decide(&req, &FleetSnapshot::empty(), now, &origin()?)?;
    assert!(matches!(
        initial.decision,
        RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        }
    ));
    assert_eq!(
        initial.feed.len(),
        2,
        "workspace and ledger are both feed records"
    );
    let mut snapshot = FleetSnapshot::empty();
    for write in &initial.writes {
        match write {
            FleetWrite::Put(record) => snapshot.records.push(record.as_ref().clone()),
            FleetWrite::Ledger(ledger) => snapshot.ledger = Some(ledger.as_ref().clone()),
            FleetWrite::DecisionReceipt { effect, .. } => {
                snapshot.decision_receipts.push(effect.clone())
            }
            _ => {}
        }
    }
    let replay = decide(&req, &snapshot, now, &origin()?)?;
    assert_eq!(replay.decision, initial.decision);
    assert!(replay.writes.is_empty() && replay.feed.is_empty());
    let mut altered = req.clone();
    altered.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Changed".to_owned())?,
        description: Description::try_from("Board work".to_owned())?,
    };
    assert!(matches!(
        decide(&altered, &snapshot, now, &origin()?),
        Err(FleetAdmissionError::ConflictingRequest)
    ));
    snapshot.ledger = None;
    let duplicate = decide(&req, &snapshot, now, &origin()?)?;
    assert!(
        matches!(duplicate.decision, RequestDecision::Rejected { reasons } if matches!(reasons.as_slice(), [Refusal::AlreadyExists(_)]))
    );
    snapshot.records.clear();
    snapshot.names.push(FleetNameOwner {
        scope: FleetNameScope::Workspaces {},
        name: ResourceName::try_from("Router".to_owned())?,
        record: WorkspaceId::generate().into(),
    });
    let collision = decide(&req, &snapshot, now, &origin()?)?;
    assert!(
        matches!(collision.decision, RequestDecision::Rejected { reasons } if matches!(reasons.as_slice(), [Refusal::NameConflict { .. }]))
    );
    Ok(())
}
