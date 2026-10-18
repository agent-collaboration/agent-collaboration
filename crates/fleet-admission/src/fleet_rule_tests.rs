#![allow(clippy::panic_in_result_fn)]
use crate::*;
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

pub(crate) fn request(
    operation: FleetOperation,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace: WorkspaceId::try_from("01890f2e-7b4c-7cc0-98c4-000000000100".to_owned())?,
        actor: RecordedActor {
            actor: message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("fixture machine".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}

#[test]
fn create_replay_conflict_and_name_collision_are_decision_table_cases() -> TestResult {
    let req = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Router".to_owned())?,
        description: Description::try_from("Board work".to_owned())?,
    })?;
    let now = "2026-10-12T00:00:00Z".parse()?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let initial = decide(&req, &FleetSnapshot::empty(), now, &ids)?;
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
            _ => {}
        }
    }
    let replay = decide(&req, &snapshot, now, &ids)?;
    assert_eq!(replay.decision, initial.decision);
    assert!(replay.writes.is_empty() && replay.feed.is_empty());
    let mut altered = req.clone();
    altered.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Changed".to_owned())?,
        description: Description::try_from("Board work".to_owned())?,
    };
    assert!(matches!(
        decide(&altered, &snapshot, now, &ids),
        Err(FleetAdmissionError::ConflictingRequest)
    ));
    snapshot.ledger = None;
    let duplicate = decide(&req, &snapshot, now, &ids)?;
    assert!(
        matches!(duplicate.decision, RequestDecision::Rejected { reasons } if matches!(reasons.as_slice(), [Refusal::AlreadyExists(_)]))
    );
    snapshot.records.clear();
    snapshot.names.push(FleetNameOwner {
        scope: FleetNameScope::Workspaces {},
        name: ResourceName::try_from("Router".to_owned())?,
        record: WorkspaceId::generate().into(),
    });
    let collision = decide(&req, &snapshot, now, &ids)?;
    assert!(
        matches!(collision.decision, RequestDecision::Rejected { reasons } if matches!(reasons.as_slice(), [Refusal::NameConflict { .. }]))
    );
    Ok(())
}

fn definition_input(source: &str) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    serde_json::json!({"reference":{"name":"workflow","version":"one",
        "sourceHash":format!("{:x}",Sha256::digest(source.as_bytes()))},"source":source})
}

fn workspace_snapshot() -> Result<FleetSnapshot, Box<dyn std::error::Error + Send + Sync>> {
    let create = request(FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("work".to_owned())?,
        description: Description::try_from("source proof".to_owned())?,
    })?;
    let decision = decide(
        &create,
        &FleetSnapshot::empty(),
        "2026-10-12T00:00:00Z".parse()?,
        &FleetIds {
            generated_record: RecordId::generate(),
        },
    )?;
    let mut snapshot = FleetSnapshot::empty();
    snapshot.records = decision
        .writes
        .into_iter()
        .filter_map(|write| match write {
            FleetWrite::Put(record) => Some(*record),
            _ => None,
        })
        .collect();
    snapshot.high_water = FleetPosition::try_from(2)?;
    Ok(snapshot)
}

#[test]
fn registered_behavior_is_parsed_from_the_exact_source_document() -> TestResult {
    let source = r#"{"initialStatus":"source-state","edges":[{"name":"advance","from":"source-state","to":"done","requiredRole":"lead","guards":[]}]}"#;
    let parsed = serde_json::from_value::<ProcessDefinitionInput>(definition_input(source));
    assert!(parsed.is_ok(), "source-only registration: {parsed:?}");
    let req = request(FleetOperation::DefinitionRegister {
        definition: parsed?,
    })?;
    let decision = decide(
        &req,
        &workspace_snapshot()?,
        "2026-10-12T00:00:00Z".parse()?,
        &FleetIds {
            generated_record: RecordId::generate(),
        },
    )?;
    let definition = decision
        .writes
        .iter()
        .find_map(|write| match write {
            FleetWrite::Put(record) => match record.as_ref() {
                Record::ProcessDefinition(definition) => Some(definition),
                _ => None,
            },
            _ => None,
        })
        .expect("parsed definition");
    assert_eq!(definition.source, source);
    assert_eq!(definition.initial_status.as_str(), "source-state");
    assert_eq!(definition.edges.len(), 1);
    assert_eq!(definition.edges[0].name.as_str(), "advance");
    Ok(())
}

#[test]
fn registration_cannot_supply_a_graph_independent_of_the_hashed_source() {
    let mut input = definition_input(r#"{"initialStatus":"new","edges":[]}"#);
    input["initialStatus"] = serde_json::json!("forged-state");
    input["edges"] = serde_json::json!([]);
    assert!(serde_json::from_value::<ProcessDefinitionInput>(input).is_err());
}

#[test]
fn unknown_source_document_fields_are_rejected_in_a_replayable_decision() -> TestResult {
    let source = r#"{"initialStatus":"new","edges":[],"allowAnything":true}"#;
    let parsed = serde_json::from_value::<ProcessDefinitionInput>(definition_input(source));
    assert!(parsed.is_ok(), "raw registration shape: {parsed:?}");
    let req = request(FleetOperation::DefinitionRegister {
        definition: parsed?,
    })?;
    let snapshot = workspace_snapshot()?;
    let now = "2026-10-12T00:00:00Z".parse()?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let rejected = decide(&req, &snapshot, now, &ids)?;
    assert!(
        matches!(&rejected.decision, RequestDecision::Rejected { reasons }
        if matches!(reasons.as_slice(), [Refusal::InvalidShape { field }] if field == "source"))
    );
    assert_eq!(
        rejected.feed.len(),
        1,
        "only the rejected ledger record is written"
    );
    let mut replay_snapshot = snapshot;
    replay_snapshot.ledger = rejected.writes.iter().find_map(|write| match write {
        FleetWrite::Ledger(ledger) => Some(ledger.as_ref().clone()),
        _ => None,
    });
    let replay = decide(&req, &replay_snapshot, now, &ids)?;
    assert_eq!(replay.decision, rejected.decision);
    assert!(replay.writes.is_empty() && replay.feed.is_empty());
    Ok(())
}

#[test]
fn project_move_definition_refuses_workstream_authority_roles() -> TestResult {
    for role in ["implementer", "verifier"] {
        let source = format!(
            r#"{{"initialStatus":"new","edges":[{{"name":"advance","from":"new","to":"done","requiredRole":"{role}","guards":[]}}]}}"#
        );
        let definition =
            serde_json::from_value::<ProcessDefinitionInput>(definition_input(&source))?;
        let req = request(FleetOperation::DefinitionRegister { definition })?;
        let decision = decide(
            &req,
            &workspace_snapshot()?,
            "2026-10-12T00:00:00Z".parse()?,
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )?;
        assert!(
            matches!(decision.decision, RequestDecision::Rejected { reasons }
            if matches!(reasons.as_slice(), [Refusal::InvalidShape { field }] if field == "requiredRole")),
            "{role} must not become project authority"
        );
    }
    Ok(())
}
