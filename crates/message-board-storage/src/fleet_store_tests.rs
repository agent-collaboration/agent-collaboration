use crate::FleetStore;
use fleet_admission::{FleetIds, FleetOperation, FleetRequest};
use message_board::*;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn fleet_current_record_refuses_noncanonical_actor_metadata() -> TestResult {
    use sqlx::{ConnectOptions, Connection};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let store = FleetStore::open(&path).await?;
    let workspace = WorkspaceId::generate();
    let create = request(
        workspace.clone(),
        FleetOperation::WorkspaceCreate {
            name: "canonical workspace".to_owned().try_into()?,
            description: "".to_owned().try_into()?,
        },
    )?;
    store
        .admit(
            &create,
            chrono::Utc::now(),
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE fleet_records SET payload = json_set(payload, '$.record.envelope.actor.machine', ' native fixture ') WHERE kind = 'workspace' AND id = ?")
        .bind(workspace.as_str()).execute(&mut observer).await?;
    assert_eq!(changed.rows_affected(), 1);
    assert!(
        store.record(&workspace.into()).await.is_err(),
        "current fleet record normalized corrupt stored actor metadata"
    );
    observer.close().await?;
    store.close().await;
    Ok(())
}

#[tokio::test]
async fn fleet_feed_refuses_noncanonical_body_text_without_changing_canonical_records() -> TestResult
{
    use sqlx::{ConnectOptions, Connection};
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let store = FleetStore::open(&path).await?;
    let workspace = WorkspaceId::generate();
    let create = request(
        workspace.clone(),
        FleetOperation::WorkspaceCreate {
            name: "canonical workspace".to_owned().try_into()?,
            description: "".to_owned().try_into()?,
        },
    )?;
    store
        .admit(
            &create,
            chrono::Utc::now(),
            &FleetIds {
                generated_record: RecordId::generate(),
            },
        )
        .await?;
    let canonical = store.feed_after(FleetPosition::ZERO, 1).await?;
    assert_eq!(canonical.entries.len(), 1);
    let original = serde_json::to_value(canonical.entries.first().ok_or("entry")?)?;
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    for (pointer, padded) in [
        ("/record/record/name", " canonical workspace "),
        ("/record/record/envelope/actor/machine", " native fixture "),
    ] {
        let mut corrupt = original.clone();
        *corrupt.pointer_mut(pointer).ok_or("stored field")? = serde_json::json!(padded);
        let changed = sqlx::query("UPDATE fleet_feed SET entry = ? WHERE position = 1")
            .bind(corrupt.to_string())
            .execute(&mut observer)
            .await?;
        assert_eq!(changed.rows_affected(), 1);
        assert!(
            store.feed_after(FleetPosition::ZERO, 1).await.is_err(),
            "fleet feed normalized stored text at {pointer}"
        );
        assert!(
            matches!(store.record(&workspace.clone().into()).await?, Some(Record::Workspace(row)) if row.name.as_str() == "canonical workspace")
        );
        sqlx::query("UPDATE fleet_feed SET entry = ? WHERE position = 1")
            .bind(original.to_string())
            .execute(&mut observer)
            .await?;
    }
    observer.close().await?;
    assert_eq!(store.feed_after(FleetPosition::ZERO, 1).await?, canonical);
    store.close().await;
    Ok(())
}

fn request(
    workspace: WorkspaceId,
    operation: FleetOperation,
) -> Result<FleetRequest, Box<dyn std::error::Error + Send + Sync>> {
    Ok(FleetRequest {
        request_id: RequestId::generate(),
        workspace,
        actor: RecordedActor {
            actor: message_board::ActorKind::Declared(Identity::Human {
                human_id: HumanId::try_from("owner".to_owned())?,
            }),
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("native fixture".to_owned())?,
        },
        acting_for: None,
        resubmits: None,
        protocol_version: ProtocolVersion::CURRENT,
        operation,
    })
}

#[tokio::test]
async fn native_fleet_replays_original_record_and_decision_after_rename_and_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("fleet.sqlite");
    let store = FleetStore::open(&path).await?;
    let workspace = WorkspaceId::generate();
    let original = request(
        workspace.clone(),
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Original".to_owned())?,
            description: Description::try_from("native fleet".to_owned())?,
        },
    )?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let now = "2026-10-12T00:00:00Z".parse()?;
    let first = store.admit(&original, now, &ids).await?;
    assert!(matches!(
        first.result,
        RequestResult::Decided(RequestDecision::Accepted {
            change: ChangeOutcome::Changed,
            ..
        })
    ));
    assert_eq!(store.snapshot_records().await?.through.get(), 2);
    let rename = request(
        workspace.clone(),
        FleetOperation::WorkspaceUpdate {
            name: ResourceName::try_from("Renamed".to_owned())?,
            description: Description::try_from("native fleet".to_owned())?,
        },
    )?;
    store.admit(&rename, now, &ids).await?;
    assert_eq!(store.snapshot_records().await?.through.get(), 4);
    store.close().await;
    let reopened = FleetStore::open(&path).await?;
    let replay = reopened.admit(&original, now, &ids).await?;
    assert_eq!(replay.result, first.result);
    assert_eq!(
        replay.records, first.records,
        "replay carries the first accepted record, not a later rename"
    );
    // Only the historical version is corrupt: the current workspace is Renamed,
    // so replay must take reply_for's feed fallback rather than its snapshot.
    use sqlx::{ConnectOptions, Connection};
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let reference: RecordRef = workspace.clone().into();
    let original_record = first
        .records
        .iter()
        .find(|row| row.reference() == reference)
        .ok_or("original workspace")?;
    let RecordPosition::Fleet(position) = original_record.envelope().position else {
        return Err("fleet position".into());
    };
    let position = i64::try_from(position.get())?;
    let encoded: String = sqlx::query_scalar("SELECT entry FROM fleet_feed WHERE position=?")
        .bind(position)
        .fetch_one(&mut observer)
        .await?;
    let mut corrupt: serde_json::Value = serde_json::from_str(&encoded)?;
    let name = corrupt
        .pointer_mut("/record/record/name")
        .ok_or("historical name")?;
    assert_eq!(name, &serde_json::json!("Original"));
    *name = serde_json::json!(" Original ");
    let before_corruption = reopened.snapshot_records().await?;
    let changed = sqlx::query("UPDATE fleet_feed SET entry=? WHERE position=?")
        .bind(corrupt.to_string())
        .bind(position)
        .execute(&mut observer)
        .await?;
    assert_eq!(changed.rows_affected(), 1);
    let refused = reopened.admit(&original, now, &ids).await;
    assert!(
        matches!(&refused, Err(crate::FleetStorageError::InvalidRecordAt {record}) if record == &reference),
        "noncanonical historical replay was accepted: {refused:?}"
    );
    let after_refusal = reopened.snapshot_records().await?;
    assert_eq!(after_refusal.through, before_corruption.through);
    assert_eq!(
        after_refusal.records, before_corruption.records,
        "failed replay must change no current record or ledger"
    );
    let repaired = sqlx::query("UPDATE fleet_feed SET entry=? WHERE position=?")
        .bind(encoded)
        .bind(position)
        .execute(&mut observer)
        .await?;
    assert_eq!(repaired.rows_affected(), 1);
    assert_eq!(reopened.admit(&original, now, &ids).await?, replay);
    observer.close().await?;
    let mut conflict = original.clone();
    conflict.operation = rename.operation.clone();
    assert_eq!(
        reopened.admit(&conflict, now, &ids).await?.result,
        RequestResult::ConflictingRequest
    );
    assert_eq!(
        reopened.snapshot_records().await?.through.get(),
        4,
        "replay/conflict consume no new positions"
    );
    let mut duplicate = original.clone();
    duplicate.request_id = RequestId::generate();
    assert!(
        matches!(reopened.admit(&duplicate, now, &ids).await?.result, RequestResult::Decided(RequestDecision::Rejected { reasons }) if matches!(reasons.as_slice(), [Refusal::AlreadyExists(record)] if record == &workspace.clone().into()))
    );
    let collision = request(WorkspaceId::generate(), rename.operation.clone())?;
    // A fresh workspace creation with the occupied name preserves nameConflict.
    let mut collision = collision;
    collision.operation = FleetOperation::WorkspaceCreate {
        name: ResourceName::try_from("Renamed".to_owned())?,
        description: Description::try_from("native fleet".to_owned())?,
    };
    let rejected = reopened.admit(&collision, now, &ids).await?;
    assert!(
        matches!(&rejected.result, RequestResult::Decided(RequestDecision::Rejected { reasons }) if matches!(reasons.as_slice(), [Refusal::NameConflict { existing, .. }] if existing == &workspace.clone().into()))
    );
    assert_eq!(reopened.admit(&collision, now, &ids).await?, rejected);
    assert_eq!(reopened.snapshot_records().await?.through.get(), 6);
    reopened.close().await;
    Ok(())
}

#[tokio::test]
async fn shared_fleet_decision_tables_run_on_native_turso() -> TestResult {
    use fleet_admission::conformance_cases::{
        FleetCaseAction, FleetExpectation, FleetRecordExpectation, cases,
    };
    for case in cases()? {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("fleet.sqlite");
        let mut store = FleetStore::open(&path).await?;
        let now = "2026-10-12T00:00:00Z".parse()?;
        let mut decisions = Vec::new();
        for (index, step) in case.steps.iter().enumerate() {
            let ids = FleetIds {
                generated_record: RecordId::try_from(format!(
                    "01890f2e-7b4c-7cc0-98c4-{:012x}",
                    1000 + index
                ))?,
            };
            let before = store.snapshot_records().await?.through;
            let decision = match &step.action {
                FleetCaseAction::Request {
                    request,
                    expectation,
                } => {
                    let reply = store.admit(request, now, &ids).await?;
                    match expectation {
                        FleetExpectation::Accepted(expected) => assert!(
                            matches!(&reply.result, RequestResult::Decided(RequestDecision::Accepted { change, .. }) if change == expected),
                            "{} step {index}: {:?}",
                            case.name,
                            reply.result
                        ),
                        FleetExpectation::Rejected(expected) => assert!(
                            matches!(&reply.result, RequestResult::Decided(RequestDecision::Rejected { reasons }) if reasons == &vec![expected.clone()]),
                            "{} step {index}: {:?}",
                            case.name,
                            reply.result
                        ),
                        FleetExpectation::Replay { step: original } => {
                            assert_eq!(
                                Some(&reply.result),
                                decisions.get(*original).and_then(Option::as_ref),
                                "{} step {index}",
                                case.name
                            );
                            assert_eq!(store.snapshot_records().await?.through, before);
                        }
                        FleetExpectation::Conflicting => {
                            assert_eq!(
                                reply.result,
                                RequestResult::ConflictingRequest,
                                "{} step {index}",
                                case.name
                            );
                            assert_eq!(store.snapshot_records().await?.through, before);
                        }
                    }
                    Some(reply.result)
                }
                FleetCaseAction::ArchiveReport { report, expected } => {
                    assert_eq!(
                        &store.report_archive_seen(report).await?,
                        expected,
                        "{} step {index}",
                        case.name
                    );
                    None
                }
            };
            if let Some(expected) = &step.record {
                match expected {
                    FleetRecordExpectation::BoardState { board, state } => assert!(
                        matches!(store.record(&board.clone().into()).await?, Some(Record::Board(row)) if row.state == *state),
                        "{} step {index}",
                        case.name
                    ),
                    FleetRecordExpectation::AdrState { adr, status, text } => assert!(
                        matches!(store.record(&adr.clone().into()).await?, Some(Record::Adr(row)) if row.status == *status && row.text == *text),
                        "{} step {index}",
                        case.name
                    ),
                }
            }
            decisions.push(decision);
            if step.reopen_after {
                store.close().await;
                store = FleetStore::open(&path).await?;
            }
        }
        store.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn a_supplied_definition_id_collision_rolls_back_without_overwriting_content() -> TestResult {
    use sha2::{Digest, Sha256};
    let directory = tempfile::tempdir()?;
    let store = FleetStore::open(&directory.path().join("fleet.sqlite")).await?;
    let workspace = WorkspaceId::generate();
    let now = "2026-10-12T00:00:00Z".parse()?;
    let ids = FleetIds {
        generated_record: RecordId::generate(),
    };
    let create = request(
        workspace.clone(),
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from("Root".to_owned())?,
            description: Description::try_from("definitions".to_owned())?,
        },
    )?;
    store.admit(&create, now, &ids).await?;
    let definition = |name: &str,
                      source: &str|
     -> Result<
        fleet_admission::ProcessDefinitionInput,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Ok(fleet_admission::ProcessDefinitionInput {
            reference: ProcessDefinitionRef {
                name: ResourceName::try_from(name.to_owned())?,
                version: ResourceName::try_from("one".to_owned())?,
                source_hash: CanonicalHash::try_from(format!(
                    "{:x}",
                    Sha256::digest(source.as_bytes())
                ))?,
            },
            source: source.to_owned(),
        })
    };
    let first = request(
        workspace.clone(),
        FleetOperation::DefinitionRegister {
            definition: definition("first", r#"{"initialStatus":"new","edges":[]}"#)?,
        },
    )?;
    store.admit(&first, now, &ids).await?;
    let before = store.snapshot_records().await?;
    let second = request(
        workspace,
        FleetOperation::DefinitionRegister {
            definition: definition("second", r#"{"initialStatus":"other","edges":[]}"#)?,
        },
    )?;
    assert!(matches!(
        store.admit(&second, now, &ids).await,
        Err(crate::FleetStorageError::Database(_))
    ));
    assert_eq!(store.snapshot_records().await?, before);
    store.close().await;
    Ok(())
}
