//! Real library admin import and native board snapshot, then the actual Host claim/install path.
use super::*;
use board_sync_protocol::{ProjectClaimReply, ProjectClaimRequest};
use fleet_sync_protocol::*;
use fleet_sync_writer::{ReplicatedDatabase, WriterOptions};

pub(in crate::collaboration_runtime::board_composition) async fn imported_snapshot(
    central: &board_central_test_support::TestCentral,
    path: &Path,
    project: &ProjectId,
) -> Result<(SnapshotManifest, Record), Box<dyn std::error::Error + Send + Sync>> {
    let (manifest, pages, expected) = captured_board_snapshot(central, path, project).await?;
    assert_eq!(
        central
            .import_board_snapshot(pages, manifest.clone())
            .await?,
        manifest.position
    );
    Ok((manifest, expected))
}

pub(in crate::collaboration_runtime::board_composition) async fn captured_board_snapshot(
    central: &board_central_test_support::TestCentral,
    path: &Path,
    project: &ProjectId,
) -> Result<(SnapshotManifest, Vec<SnapshotPage>, Record), Box<dyn std::error::Error + Send + Sync>>
{
    let database = DatabaseId::try_from(project.as_str().to_owned())?;
    let native = Arc::new(
        ReplicatedDatabase::open_with(
            path,
            database,
            board_fleet_sync::BoardProjectSchema::new()?,
            fleet_sync_lease::LeaseHolder::provisional(
                database,
                central.session_inputs()?.peer,
                board_fleet_sync::BOARD_LEASE_POLICY,
            ),
            Arc::new(SystemClock),
            WriterOptions {
                allow_provisional_writes: true,
            },
        )
        .await?,
    );
    let source =
        message_board_storage::ProjectStore::from_database(project.clone(), native.clone()).await?;
    let mut write = source.begin_admission(origin()?).await?;
    let high_water = write.high_water().await?;
    let record = Record::Project(ProjectState {
        envelope: RecordEnvelope {
            id: project.clone().into(),
            scope: RecordScope {
                workspace: WorkspaceId::try_from(
                    "019f0000-0000-7000-8000-000000005593".to_owned(),
                )?,
                board: Some(BoardId::try_from(
                    "019f0000-0000-7000-8000-000000005594".to_owned(),
                )?),
                project: Some(project.clone()),
            },
            actor: RecordedActor::declared(Identity::Human {
                human_id: HumanId::try_from("claim-import-owner".to_owned())?,
            }),
            request: None,
            position: write.record_placement(high_water),
            time: RecordTime::Router(chrono::Utc::now()),
        },
        project_id: project.clone(),
        description: Description::try_from("real imported prefix".to_owned())?,
        revision: SubjectRevision::INITIAL,
        closed_at: None,
    });
    write.append_record(record).await?;
    write.commit().await?;
    let mut read = source.snapshot().await?;
    let expected = read
        .record(&project.clone().into())
        .await?
        .ok_or("source project record")?;
    read.finish().await?;
    let mut snapshot = native.snapshot().await?;
    let manifest = snapshot.manifest().await?;
    let mut cursor = None;
    let mut pages = Vec::new();
    loop {
        let page = snapshot.page(cursor.clone()).await?;
        cursor = page.next.clone();
        pages.push(page);
        if cursor.is_none() {
            break;
        }
    }
    snapshot.close().await?;
    source.close().await;
    Ok((manifest, pages, expected))
}

#[tokio::test]
async fn imported_claim_seeds_verifies_renews_then_publishes_before_first_board_write() -> TestResult
{
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let central = board_central_test_support::TestCentral::start(
        &directory.path().join("central"),
        &origin()?,
    )
    .await?;
    let project = ProjectId::generate();
    let (prefix, expected) =
        imported_snapshot(&central, &directory.path().join("source.sqlite"), &project).await?;
    let board = open_board_admission(&root, identity()?, Some(central.session_inputs()?)).await?;
    let registry = board
        .session
        .as_ref()
        .ok_or("session")?
        .projects
        .registry()?;
    let claim = board.claim.as_ref().ok_or("claim port")?.clone();
    let ready = board
        .session
        .as_ref()
        .ok_or("session")?
        .welcomed
        .subscribe();
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    wait_welcome(ready).await?;
    assert!(registry.existing_project(&project).await?.is_none());
    let request = ProjectClaimRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        project_id: project.clone(),
    };
    use std::os::unix::fs::PermissionsExt;
    let api_directory = directory.path().join("api");
    std::fs::create_dir(&api_directory)?;
    std::fs::set_permissions(&api_directory, std::fs::Permissions::from_mode(0o700))?;
    let app = (collaboration_service::ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(admission.clone())
    .with_project_claim(claim.clone()))
    .build();
    let api = collaboration_mcp::test_support::ServedCollaborationApi::start(&api_directory, app)?;
    let client = api.client("claim-proof").await?;
    let mut unsupported = request.clone();
    unsupported.protocol_version = ProtocolVersion::try_from(2_u64)?;
    assert!(matches!(client.project_claim(unsupported).await,
        Err(collaboration_client::BoardClientError::Rejected(error))
        if error.kind == BoardFailureKind::InvalidField
            && error.stage == BoardFailureStage::Validation
            && matches!(&error.details, BoardErrorDetails::FieldConstraint { field, .. } if field == "protocolVersion")));
    let answer = timeout(BOUND, async {
        loop {
            let answer = client.project_claim(request.clone()).await?;
            if !matches!(answer, ProjectClaimReply::Pending { .. }) {
                return Ok::<_, collaboration_client::BoardClientError>(answer);
            }
        }
    })
    .await??;
    let ProjectClaimReply::Claimed { epoch, start, .. } = answer else {
        return Err(format!("claim did not publish: {answer:?}").into());
    };
    assert_eq!(epoch, Epoch::new(2)?);
    assert_eq!(start, prefix.position);
    let store = registry
        .existing_project(&project)
        .await?
        .ok_or("claimed project was not published")?;
    let mut read = store.snapshot().await?;
    assert_eq!(
        read.record(&project.clone().into()).await?,
        Some(expected.clone())
    );
    read.finish().await?;
    let database = registry.database(&project).await?;
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("verified seed receipt")?;
    assert!(receipt.verified && receipt.ready);
    assert_eq!(receipt.install.position, prefix.position);
    assert_eq!(receipt.install.schema, prefix.schema);
    let mut local = database.snapshot().await?;
    assert_eq!(local.manifest().await?.tables, prefix.tables);
    local.close().await?;
    let replay = timeout(BOUND, client.project_claim(request.clone())).await??;
    assert_eq!(replay, answer);
    let mut write = store.begin_admission(origin()?).await?;
    assert_eq!(write.admitted_epoch()?.get(), 2);
    let Record::Project(mut changed) = expected else {
        return Err("source was not project record".into());
    };
    changed.description = Description::try_from("first live write on new epoch".to_owned())?;
    changed.envelope.time = RecordTime::Router(chrono::Utc::now());
    let appended = write.append_record(Record::Project(changed)).await?;
    write.commit().await?;
    assert_eq!(appended.epoch.get(), 2);
    drop(store);
    drop(database);
    drop(client);
    api.stop().await?;
    drop(claim);
    drop(registry);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;
    central.shutdown().await?;
    Ok(())
}
