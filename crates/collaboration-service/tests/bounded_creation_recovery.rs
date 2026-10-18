//! Creation replay and startup recovery load creation operands, not retained facts/ledgers.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{ConnectOptions, Connection};
use std::collections::BTreeMap;

async fn seed_unpublished_project(
    fixture: &BoardFixture,
    size: usize,
) -> Result<(ProjectCreateRequest, RecordRef, RecordRef), TestError> {
    let (published, _) = fixture.project().await?;
    let published_store = fixture
        ._registry
        .existing_project(&published)
        .await?
        .ok_or("published store")?;
    let mut read = published_store.snapshot().await?;
    let template = read
        .record(&published.clone().into())
        .await?
        .ok_or("project template")?;
    read.finish().await?;
    let request = ProjectCreateRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: None,
        project_id: ProjectId::generate(),
        board_id: fixture.board.clone(),
        title: "unpublished project".to_owned().try_into()?,
        actor: serde_json::from_value(fixture.actor.clone())?,
        acting_for: None,
    };
    let scope = RecordScope {
        workspace: fixture.workspace.clone(),
        board: Some(fixture.board.clone()),
        project: Some(request.project_id.clone()),
    };
    let envelope = |id| RecordEnvelope {
        id,
        scope: scope.clone(),
        request: Some(request.request_id.clone()),
        ..template.envelope().clone()
    };
    let project = Record::Project(ProjectState {
        envelope: envelope(request.project_id.clone().into()),
        project_id: request.project_id.clone(),
        status: None,
        pinned_definition: None,
        revision: SubjectRevision::INITIAL,
        closed_at: None,
    });
    let fact_id = FactId::generate();
    let creation = Record::Fact(Fact {
        envelope: envelope(fact_id.clone().into()),
        fact_id,
        kind: FactKind::Lifecycle(LifecycleFact::ProjectCreated {
            entry: Box::new(ProjectDirectoryEntry {
                envelope: RecordEnvelope {
                    scope: RecordScope {
                        project: None,
                        ..scope.clone()
                    },
                    position: RecordPosition::Fleet(FleetPosition::ZERO),
                    ..envelope(request.project_id.clone().into())
                },
                id: request.project_id.clone(),
                board: request.board_id.clone(),
                title: request.title.clone(),
            }),
            creating_request: request.request_id.clone(),
            acting_for: None,
        }),
        links: vec![request.project_id.clone().into()],
        producer_time: template.envelope().router_time,
        anchor: None,
        late: false,
    });
    let bytes = serde_json::to_vec(&(
        &request.project_id,
        &request.board_id,
        &request.title,
        &request.actor,
        &request.acting_for,
        &request.resubmits,
        request.protocol_version,
    ))?;
    let hash = CanonicalHash::try_from(format!("{:x}", Sha256::digest(bytes)))?;
    let store = fixture._registry.open_project(&request.project_id).await?;
    let mut write = store.begin_admission().await?;
    write
        .record_writer_epoch(&WriterEpoch {
            project: request.project_id.clone(),
            number: EpochNumber::try_from(1)?,
            holder: template.envelope().actor.installation.clone(),
            start_position: FeedPosition::ZERO,
            end: None,
        })
        .await?;
    let references = vec![project.reference(), creation.reference()];
    let project_position = write.append_record(project).await?;
    let fact_position = write.append_record(creation).await?;
    write
        .append_record(Record::Request(RequestLedgerEntry {
            envelope: envelope(request.request_id.clone().into()),
            request_id: request.request_id.clone(),
            canonical_hash: hash.clone(),
            observed_epoch: None,
            accepted_in: Some(project_position.epoch),
            decision: RequestDecision::Accepted {
                records: references,
                positions: vec![
                    RecordPosition::Project(project_position),
                    RecordPosition::Project(fact_position),
                ],
                change: ChangeOutcome::Changed,
            },
            resubmits: None,
        }))
        .await?;
    let mut first = None;
    for number in 0..size {
        let fact_id = FactId::generate();
        let fact = Record::Fact(Fact {
            envelope: RecordEnvelope {
                request: None,
                ..envelope(fact_id.clone().into())
            },
            fact_id,
            kind: FactKind::EvaluatorResult {
                guard: format!("unused {number}"),
                subject: request.project_id.clone().into(),
                outcome: GuardOutcome::Met,
                evidence: Vec::new(),
                evaluator: EvaluatorRef {
                    actor: template.envelope().actor.clone(),
                    engine: None,
                    calibration: None,
                },
                executor: None,
            },
            links: vec![request.project_id.clone().into()],
            producer_time: template.envelope().router_time,
            anchor: None,
            late: false,
        });
        let request_id = RequestId::generate();
        let rejected = Record::Request(RequestLedgerEntry {
            envelope: RecordEnvelope {
                request: Some(request_id.clone()),
                ..envelope(request_id.clone().into())
            },
            request_id,
            canonical_hash: hash.clone(),
            observed_epoch: Some(project_position.epoch),
            accepted_in: Some(project_position.epoch),
            decision: RequestDecision::Rejected {
                reasons: vec![Refusal::Archived],
            },
            resubmits: None,
        });
        first.get_or_insert_with(|| (fact.reference(), rejected.reference()));
        write.append_record(fact).await?;
        write.append_record(rejected).await?;
    }
    write.commit().await?;
    let (fact, ledger) = first.ok_or("empty history")?;
    Ok((request, fact, ledger))
}

#[allow(clippy::panic_in_result_fn)]
async fn recovery_costs(size: usize) -> Result<BTreeMap<&'static str, ReadCost>, TestError> {
    let fixture = BoardFixture::start().await?;
    let (request, unrelated_fact, unrelated_ledger) =
        seed_unpublished_project(&fixture, size).await?;
    let store = fixture
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("store")?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    for (reference, path) in [
        (
            &unrelated_fact,
            "$.record.kind.fact.evaluator.actor.machine",
        ),
        (&unrelated_ledger, "$.record.canonicalHash"),
    ] {
        let changed = sqlx::query(
            "UPDATE record_identities SET payload = json_set(payload, ?, '') WHERE id = ?",
        )
        .bind(path)
        .bind(reference.id.as_str())
        .execute(&mut connection)
        .await?;
        assert_eq!(changed.rows_affected(), 1);
    }
    connection.close().await?;
    let before = current_cost();
    fixture._admission.recover_creating_projects().await?;
    let recovery = current_cost().since(before);
    let directory = fixture
        .api
        .call("project_show", json!({"projectId":request.project_id}))
        .await?;
    assert_eq!(
        directory.pointer("/result/project/id"),
        Some(&json!(request.project_id)),
        "startup failed to publish the creation operands beside unrelated corrupt history: {directory}"
    );
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    let before = current_cost();
    let reply = fixture._admission.create_project(&request).await?;
    let replay = current_cost().since(before);
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.high_water(),
        head,
        "creation replay repeated project effects"
    );
    assert!(read.record(&unrelated_fact).await.is_err());
    assert!(read.record(&unrelated_ledger).await.is_err());
    read.finish().await?;
    fixture.stop().await?;
    Ok(BTreeMap::from([
        ("startup", recovery),
        ("creation replay", replay),
    ]))
}

#[derive(Clone, Copy)]
enum CreationRedirect {
    ProjectIdentity,
    ProjectHome,
}

#[allow(clippy::panic_in_result_fn)]
async fn creation_operand_cannot_redirect_the_project(
    redirect: CreationRedirect,
) -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (request, _, _) = seed_unpublished_project(&fixture, 1).await?;
    let store = fixture
        ._registry
        .existing_project(&request.project_id)
        .await?
        .ok_or("store")?;
    let foreign = ProjectId::generate();
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    let changed = match redirect {
        CreationRedirect::ProjectIdentity => sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.kind.fact.entry.id', ?) WHERE kind = 'fact' AND json_extract(payload, '$.record.kind.kind') = 'lifecycle' AND json_extract(payload, '$.record.kind.fact.kind') = 'projectCreated'")
            .bind(foreign.as_str()).execute(&mut connection).await?,
        CreationRedirect::ProjectHome => {
            let board = BoardId::generate();
            let mut create = fixture.header();
            create.as_object_mut().ok_or("header")?.extend(json!({"workspaceId":fixture.workspace,"boardId":board,"name":"other creation home","description":"","kind":{"kind":"area"}}).as_object().ok_or("fields")?.clone());
            board_fixture::accepted(&fixture.api.call("board_create", create).await?);
            sqlx::query("UPDATE record_identities SET payload = json_set(payload, '$.record.envelope.scope.board', ?, '$.record.kind.fact.entry.board', ?, '$.record.kind.fact.entry.envelope.scope.board', ?) WHERE kind = 'fact' AND json_extract(payload, '$.record.kind.kind') = 'lifecycle' AND json_extract(payload, '$.record.kind.fact.kind') = 'projectCreated'")
                .bind(board.as_str()).bind(board.as_str()).bind(board.as_str()).execute(&mut connection).await?
        }
    };
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    let reply = fixture._admission.create_project(&request).await;
    assert!(
        matches!(
            reply,
            Err(collaboration_service::ProjectAdmissionError::Storage(
                message_board_storage::ProjectStorageError::InvalidRecord {
                    record: RecordRef {
                        kind: RecordKind::Fact,
                        ..
                    },
                    ..
                }
            ))
        ),
        "creation replay accepted a directory operand for another project: {reply:?}"
    );
    let unpublished = match redirect {
        CreationRedirect::ProjectIdentity => foreign,
        CreationRedirect::ProjectHome => request.project_id,
    };
    let shown = fixture
        .api
        .call("project_show", json!({"projectId":unpublished}))
        .await?;
    assert_eq!(
        shown.pointer("/error/data/kind"),
        Some(&json!("resourceNotFound")),
        "corrupt creation published another project: {shown}"
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn startup_and_creation_replay_ignore_unrelated_fact_and_ledger_bodies()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = recovery_costs(8).await?;
    let large = recovery_costs(80).await?;
    eprintln!("creation recovery costs: small={small:?}; large={large:?}");
    assert_eq!(small, large, "recovery decoded unrelated retained history");
    for redirect in [
        CreationRedirect::ProjectIdentity,
        CreationRedirect::ProjectHome,
    ] {
        creation_operand_cannot_redirect_the_project(redirect).await?;
    }
    Ok(())
}
