//! Real holder changes distinguish the selected file epoch from application history.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    host_entrypoints::HOST_BASE_PREFIX,
    host_process::OwnedHost,
    native_snapshot_fixture::NativeFixture,
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
};
use board_central_test_support::TestCentral;
use board_fleet_sync::BoardProjectSchema;
use board_staging_cutover::{
    NativeProjectSnapshot, NativeSourceError, PreparedProjectBundle, SelectedWriterFile,
};
use board_sync_protocol::ProjectClaimReply;
use collaboration_client::{BoardClientError, CollaborationClient};
use fleet_project_writer::RetainedFile;
use fleet_protocol::{Epoch, TransferId};
use message_board::{
    ActorOrigin, BoardFailureKind, BoardOperation, CopySource, CurrentWriter, EpochNumber,
    FeedCursor, HumanId, Identity, PageLimit, ProjectRequest, ProjectViewRequest, ProtocolVersion,
    ReadActor, RequestId, TrailSubject, WorkTrailReadResult, WorkTrailSelection,
    WorkTrailShowRequest,
    public_replies::{RequestDecision, RequestResult},
};
use std::{os::unix::fs::PermissionsExt, path::Path, time::Duration};

#[tokio::test]
async fn a_real_e3_writer_with_e1_application_history_is_an_unchanged_supported_source()
-> TestResult {
    epoch_three_case(SourceHistory::InheritedOnly).await
}

#[tokio::test]
async fn a_real_e3_application_write_refuses_cutover_before_writable_source_admission() -> TestResult
{
    epoch_three_case(SourceHistory::WrittenAtEpochThree).await
}

#[derive(Clone, Copy)]
enum SourceHistory {
    InheritedOnly,
    WrittenAtEpochThree,
}

async fn epoch_three_case(history: SourceHistory) -> TestResult {
    // Arrange two real enrolled Hosts, an unchanged e1 source and actual owner HTTP publication.
    let base = tempfile::Builder::new()
        .prefix(HOST_BASE_PREFIX)
        .tempdir_in("/tmp")?;
    std::fs::set_permissions(base.path(), std::fs::Permissions::from_mode(0o700))?;
    let root = std::fs::canonicalize(base.path())?;
    let first = CutoverHostFixture::new_in(&root).map_err(boxed)?;
    let second = CutoverHostFixture::new_in(&root).map_err(boxed)?;
    let source = super::native_snapshot_fixture::native_fixture(&first)
        .await
        .map_err(boxed)?;
    let central_area = tempfile::tempdir()?;
    let central =
        TestCentral::start_with_peer(central_area.path(), &origin(&first)?, first.peer).await?;
    let second_machine = central
        .additional_machine(&origin(&second)?, second.peer)
        .await?;
    let artifacts = tempfile::tempdir()?;
    let observed = async {
        let run = RunFixture::new(&first, &source, central.base_url(), artifacts.path())?;
        let published = run.execute().await?;
        if !published.status.success() {
            return Err("actual offline publication failed".into());
        }
        let bundle = PreparedProjectBundle::open(&run.bundle)?;
        install_enrollment(&first.root, central.write_host_session(&first.root)?)?;
        install_enrollment(
            &second.root,
            second_machine.write_host_session(&second.root)?,
        )?;
        install_initial_holder(&first, &bundle).await?;
        install_next_holder(&central, &second, &source, &bundle, history)
            .await?;

        // Act only after both actual Host/native owners have stopped successfully.
        let mut offline = second.acquire().map_err(boxed)?;
        let selected = SelectedWriterFile {
            project: source.selected.project.clone(),
            peer: second.peer,
            file_epoch: Epoch::new(3)?,
        };
        if matches!(history, SourceHistory::WrittenAtEpochThree) {
            let path = codex_router_host::router_collaboration_directory(&second.root)
                .join("projects")
                .join(source.selected.project.as_str())
                .join("e3.sqlite");
            let retained = RetainedFile::open_read_only(&path, BoardProjectSchema::new()?)
                .await?;
            require(retained.origin().epoch.value()==3,"actual source file did not retain e3")?;
            let before = std::fs::read(&path)?;
            let error = match NativeProjectSnapshot::open(&mut offline, selected).await {
                Err(error) => error,
                Ok(snapshot) => {
                    snapshot.close().await?;
                    return Err("actual application epoch three was accepted".into());
                }
            };
            // Assert refusal beats the held retained reader's writable-admission conflict.
            require(
                matches!(error, NativeSourceError::UnsupportedApplicationEpoch {epoch} if epoch.get()==3),
                &format!("application epoch-three refusal lost: {error:?}")
            )?;
            require(std::fs::read(&path)? == before,"refusal changed the original source bytes")?;
            drop(retained);
        } else {
            let snapshot = NativeProjectSnapshot::open(&mut offline, selected).await?;
            // Assert N3/L1 is supported without inferring the SDK's transport position.
            let checked = async {
                snapshot.verify_replicated_tables(&bundle.manifest().header.manifest)?;
                require(snapshot.preflight().selected.file_epoch.value()==3,"selected native origin is not e3")?;
                require(snapshot.preflight().application_epoch.get()==1,"takeover appended unexplained application history")?;
                require(snapshot.preflight().high_water==source.cursor,"takeover changed application high water")?;
                require(snapshot.preflight().latest_record_at==source.record_time,"takeover changed the source record time")?;
                require(snapshot.preflight().line_two.is_none(),"takeover invented an application line")
            }.await;
            let closed = snapshot.close().await;
            checked?;
            closed?;
        }
        require(bundle.manifest().source_fingerprint.matches(&bundle.manifest().backup_directory)?,"original archived source changed")?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let machine_stopped = second_machine.shutdown().await;
    let central_stopped = central.shutdown().await;
    if let Err(error) = observed {
        let first_root = first._area.keep();
        let second_root = second._area.keep();
        let _base = base.keep();
        let bundle = artifacts.keep();
        return Err(format!(
            "{error}; owned roots {}; {}; bundle {}",
            first_root.display(),
            second_root.display(),
            bundle.display()
        )
        .into());
    }
    machine_stopped?;
    central_stopped?;
    Ok(())
}

async fn install_initial_holder(
    first: &CutoverHostFixture,
    bundle: &PreparedProjectBundle,
) -> TestResult {
    let host = OwnedHost::start(&first.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        let directory = codex_router_host::router_collaboration_directory(&first.root);
        let client = CollaborationClient::connect(&directory, "cutover-e2", "1").await?;
        let answer = super::public_claim::claim(&client, bundle).await?;
        require(
            matches!(answer, ProjectClaimReply::Claimed {epoch, ..} if epoch.value()==2),
            &format!("initial holder Claim: {answer:?}"),
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = host.stop().await;
    observed?;
    stopped?;
    Ok(())
}

async fn install_next_holder(
    central: &TestCentral,
    second: &CutoverHostFixture,
    source: &NativeFixture,
    bundle: &PreparedProjectBundle,
    history: SourceHistory,
) -> TestResult {
    let host = OwnedHost::start(&second.root, &bundle.manifest().header.declared_owner).await?;
    let observed = async {
        host.await_ready().await?;
        let taken = central
            .take_over_project(
                source.manifest.database,
                &origin(second)?.installation,
                TransferId::new(uuid::Uuid::now_v7())?,
            )
            .await?;
        require(
            taken.pointer("/value/newEpoch") == Some(&serde_json::json!(3)),
            "owner takeover did not grant e3",
        )?;
        let directory = codex_router_host::router_collaboration_directory(&second.root);
        let client = CollaborationClient::connect(&directory, "cutover-e3", "1").await?;
        // Takeover already assigned this peer; a new Claim correctly says AlreadyHeld.
        // Await the real grant-notice install through a read-only public writer view.
        await_epoch_three_view(&client, source).await?;
        if matches!(history, SourceHistory::WrittenAtEpochThree) {
            append_epoch_three_record(&client, source, &bundle.manifest().header.declared_owner)
                .await?;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let stopped = host.stop().await;
    observed?;
    stopped?;
    Ok(())
}

async fn append_epoch_three_record(
    client: &CollaborationClient,
    source: &NativeFixture,
    owner: &HumanId,
) -> TestResult {
    let request = RequestId::generate();
    let row_id: message_board::WorkTrailRowId = request.as_str().to_owned().try_into()?;
    let text = "A0 genuine application epoch-three source"
        .to_owned()
        .try_into()?;
    let written = client
        .work_trail_add(ProjectRequest {
            request_id: request.clone(),
            project_id: source.selected.project.clone(),
            actor: Identity::Human {
                human_id: owner.clone(),
            },
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation: BoardOperation::WorkTrailAdd {
                row_id: row_id.clone(),
                subject: Some(TrailSubject::Project),
                text,
                tags: vec![],
                links: vec![],
            },
        })
        .await?;
    require(
        matches!(
            written.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "actual epoch-three trail admission was not accepted",
    )?;
    let shown = client
        .work_trail_show(WorkTrailShowRequest {
            reader: ReadActor::Anonymous {},
            project_id: source.selected.project.clone(),
            selection: WorkTrailSelection::Forward {
                after: source.cursor,
            },
            limit: PageLimit::try_from(10)?,
            subject: None,
            tags: vec![],
            include_redacted: true,
        })
        .await?;
    let WorkTrailReadResult::Forward(page) = shown.value else {
        return Err("original source cursor was not retained at the real e3 holder".into());
    };
    let [row] = page.rows.as_slice() else {
        return Err("one actual e3 trail record required".into());
    };
    let expected = FeedCursor {
        epoch: EpochNumber::try_from(3)?,
        position: source
            .cursor
            .position
            .get()
            .checked_add(1)
            .ok_or("source cursor overflow")?
            .try_into()?,
    };
    require(
        row.row_id == row_id,
        "actual epoch-three row identity differs",
    )?;
    require(
        row.envelope.request.as_ref() == Some(&request),
        "actual epoch-three request identity differs",
    )?;
    require(
        row.envelope.position.project_cursor() == Some(expected),
        "actual epoch-three write did not continue the original application cursor",
    )?;
    Ok(())
}

async fn await_epoch_three_view(
    client: &CollaborationClient,
    source: &NativeFixture,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            match client
                .view_work(ProjectViewRequest {
                    project_id: source.selected.project.clone(),
                    include_removed: true,
                })
                .await
            {
                Ok(view)
                    if matches!(view.current_writer,CurrentWriter::Held {epoch} if epoch.get()==3)
                        && matches!(view.source, CopySource::Writer {})
                        && view.as_of == source.cursor =>
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                Ok(_) => tokio::task::yield_now().await,
                Err(BoardClientError::Rejected(error))
                    if error.kind == BoardFailureKind::BoardUnavailable =>
                {
                    tokio::task::yield_now().await
                }
                Err(error) => return Err(error.into()),
            }
        }
    })
    .await??;
    Ok(())
}

fn origin(host: &CutoverHostFixture) -> TestResult<ActorOrigin> {
    Ok(ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "real cutover epoch source".to_owned().try_into()?,
    })
}

fn install_enrollment(root: &Path, document: std::path::PathBuf) -> TestResult {
    std::fs::create_dir_all(root.join("fleet"))?;
    std::fs::rename(document, root.join("fleet/enrollment.json"))?;
    Ok(())
}

fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}
