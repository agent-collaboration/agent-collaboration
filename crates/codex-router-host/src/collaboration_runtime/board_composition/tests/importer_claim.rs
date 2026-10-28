//! D's real importer composes with B's actual Claim, install, writer and replication owners.
use super::*;
use board_legacy_upgrade::{UpgradePublisher, prepare_sources};
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use fleet_sync_protocol::{
    CopyStatusAnswer, OperationAnswer, OperationRequest, ReplicationAnswer, ReplicationRequest,
};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Executor};

async fn fault(root: &Path, statement: &'static str) -> TestResult {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join("fleet.db"))
        .create_if_missing(false)
        .connect()
        .await?;
    let result = connection.execute(statement).await;
    connection.close().await?;
    result?;
    Ok(())
}

#[tokio::test]
async fn actual_legacy_publication_claim_and_first_live_piece_preserve_the_imported_prefix()
-> TestResult {
    Box::pin(run_claim_after_import(false)).await
}

#[tokio::test]
async fn claim_advances_the_real_copy_before_interrupted_import_completion_resumes() -> TestResult {
    Box::pin(run_claim_after_import(true)).await
}

async fn run_claim_after_import(interrupted: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let central_root = directory.path().join("central");
    let central = board_central_test_support::TestCentral::start(&central_root, &origin()?).await?;
    let source = directory.path().join("legacy.sqlite");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../board-legacy-upgrade/tests/fixtures/legacy-complete.sqlite"),
        &source,
    )?;
    let prepared = prepare_sources(
        &[source],
        fleet_admission::ImportProvenance {
            installation: origin()?.installation,
            machine: origin()?.machine,
        },
        HumanId::try_from("legacy-owner".to_owned())?,
    )
    .await?;
    let project = prepared
        .projects
        .first()
        .ok_or("prepared project")?
        .store
        .project_id()
        .clone();
    let prefix = prepared
        .report
        .projects
        .first()
        .ok_or("report project")?
        .manifest
        .position;
    let mut read = prepared
        .projects
        .first()
        .ok_or("prepared project")?
        .store
        .snapshot()
        .await?;
    let expected = read
        .record(&project.clone().into())
        .await?
        .ok_or("source project state")?;
    read.finish().await?;
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        "Cf-Access-Jwt-Assertion",
        reqwest::header::HeaderValue::from_str(
            &collaboration_central_conformance::owner_assertion()?,
        )?,
    );
    let http = reqwest::Client::builder()
        .no_proxy()
        .default_headers(headers)
        .timeout(BOUND)
        .build()?;
    let publisher = UpgradePublisher::new(reqwest::Url::parse(central.base_url())?, http);
    if interrupted {
        // Load/reserve the real Fleet object before arming its write fault.
        let client = collaboration_central_conformance::ConformanceClient::new(central.base_url())?;
        client.import_fleet(&prepared.report.fleet).await?;
        fault(&central_root,"CREATE TRIGGER hold_import_publication BEFORE UPDATE OF phase ON admin_imports WHEN json_extract(NEW.phase,'$.kind')='published' BEGIN SELECT RAISE(ABORT,'hold completed native seed before workflow receipt'); END").await?;
        assert!(publisher.publish(&prepared).await.is_err());
    } else {
        publisher.publish(&prepared).await?;
    }
    let board = open_board_admission(
        &directory.path().join("router"),
        identity()?,
        Some(central.session_inputs()?),
    )
    .await?;
    let registry = board
        .session
        .as_ref()
        .ok_or("session")?
        .projects
        .registry()?;
    let claim = board.claim.as_ref().ok_or("claim")?.clone();
    let ready = board
        .session
        .as_ref()
        .ok_or("session")?
        .welcomed
        .subscribe();
    let admission = board.admission.clone();
    let session_requests = board.session.as_ref().ok_or("session")?.requests.clone();
    let running = board.start().ok_or("running session")?;
    wait_welcome(ready).await?;
    let before = session_requests
        .request(OperationRequest::Replication(
            ReplicationRequest::CopyStatus {
                database: project.as_str().to_owned().try_into()?,
            },
        ))
        .await?;
    assert!(
        matches!(before, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked == prefix && status.holder.is_none() && status.fence == fleet_sync_protocol::Epoch::new(1)?),
        "the actual import did not publish its exact unheld prefix before Claim"
    );
    let request = ProjectClaimRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    let answer = timeout(BOUND, async {
        loop {
            let answer = claim.claim(&request).await?;
            if !matches!(answer, ProjectClaimReply::Pending { .. }) {
                return Ok::<_, collaboration_contracts::ProjectClaimError>(answer);
            }
        }
    })
    .await??;
    let ProjectClaimReply::Claimed { epoch, start, .. } = answer else {
        return Err(format!("actual imported Claim did not complete: {answer:?}").into());
    };
    assert_eq!(epoch, fleet_sync_protocol::Epoch::new(2)?);
    assert_eq!(start, prefix);
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("published local project")?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&project.clone().into()).await?,
        Some(expected.clone())
    );
    read.finish().await?;
    let Record::Project(mut state) = expected else {
        return Err("expected project state".into());
    };
    state.description = Description::try_from("first live imported-holder write".to_owned())?;
    state.envelope.time = RecordTime::Router(chrono::Utc::now());
    let mut write = store.begin_admission(origin()?).await?;
    let appended = write.append_record(Record::Project(state)).await?;
    write.commit().await?;
    assert_eq!(appended.epoch.get(), 2);
    let database = registry.database(&project).await?;
    let mut snapshot = database.snapshot().await?;
    let through = snapshot.manifest().await?.position;
    snapshot.close().await?;
    timeout(BOUND,async {
        loop {
            let answer=session_requests.request(OperationRequest::Replication(ReplicationRequest::CopyStatus {database:project.as_str().to_owned().try_into()?})).await?;
            if matches!(answer,OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.acked >= through) {return Ok::<_,Box<dyn std::error::Error+Send+Sync>>(());}
            tokio::task::yield_now().await;
        }
    }).await??;
    if interrupted {
        fault(&central_root, "DROP TRIGGER hold_import_publication").await?;
    }
    // A retry must preserve the actual new-epoch piece and holder, rather than
    // require the Copy summary to remain forever at the old imported prefix.
    publisher.publish(&prepared).await?;
    let after = session_requests
        .request(OperationRequest::Replication(
            ReplicationRequest::CopyStatus {
                database: database.database(),
            },
        ))
        .await?;
    assert!(
        matches!(after, OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(status))) if status.fence == epoch && status.acked == through && status.holder == Some(central.session_inputs()?.peer)),
        "import completion changed the live Copy holder or prefix"
    );
    let mut snapshot = database.snapshot().await?;
    let mut cursor = None;
    loop {
        let expected = snapshot.page(cursor.clone()).await?;
        let actual = session_requests
            .request(OperationRequest::Replication(
                ReplicationRequest::SnapshotPage {
                    database: database.database(),
                    cursor,
                },
            ))
            .await?;
        let OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(
            fleet_sync_protocol::SnapshotRead::Page(actual),
        )) = actual
        else {
            return Err("advanced Copy snapshot unavailable after import completion".into());
        };
        assert_eq!(*actual, expected);
        cursor = actual.next;
        if cursor.is_none() {
            break;
        }
    }
    snapshot.close().await?;
    let mut read = store.snapshot().await?;
    let Some(Record::Project(current)) = read.record(&project.clone().into()).await? else {
        return Err("current state missing".into());
    };
    assert_eq!(
        current.description.as_str(),
        "first live imported-holder write"
    );
    read.finish().await?;
    drop(store);
    drop(database);
    drop(session_requests);
    drop(claim);
    drop(registry);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission retained"))?
        .close()
        .await?;
    prepared.close().await;
    central.shutdown().await?;
    Ok(())
}
