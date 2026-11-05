//! A genuine older file cannot authorize archiving a newer writer's unique content.
use super::{
    claim_session::ClaimSession,
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::{NativeFixture, fixture_database, native_fixture},
    offline_cli_fixture::{RunFixture, TestResult},
    recovery_rows::require,
};
use board_central_test_support::TestCentral;
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use board_staging_cutover::{NativeProjectSnapshot, PreparedProjectBundle};
use fleet_sync_protocol::{Epoch, InstallId, LeaseRefusal, RenewAnswer};
use fleet_sync_receiver::SnapshotInstall;
use fleet_sync_tokio_runner::StagingStore;
use fleet_sync_writer::{ReplicatedDatabase, RetainedAuthority};
use message_board::{
    ActorOrigin, Record, RecordId, RequestId, TrailSubject, WorkTrailRow, WorkTrailRowId,
};
use message_board_storage::ProjectStore;
use runtime_primitives::{Clock, SystemClock};
use std::sync::Arc;

const UNIQUE: &str = "unique content written only in the actual newer native file";

#[tokio::test]
async fn stale_native_selection_refuses_before_import_or_archiving_the_newer_record() -> TestResult
{
    selection_case(false).await
}
#[tokio::test]
async fn current_native_selection_exports_and_archives_the_newer_record_intact() -> TestResult {
    selection_case(true).await
}

async fn selection_case(current: bool) -> TestResult {
    // Arrange two real native epochs; only the newer file receives the extra record.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let older = native_fixture(&fixture).await.map_err(boxed)?;
    let origin = origin(&fixture)?;
    let old_area = tempfile::tempdir()?;
    let old_central = TestCentral::start_with_peer(old_area.path(), &origin, fixture.peer).await?;
    let newer = newer_writer(&fixture, &older, &old_central).await;
    let stopped = old_central.shutdown().await;
    let newer = newer?;
    stopped?;
    let selected = if current { &newer } else { &older };
    let target_area = tempfile::tempdir()?;
    let target = TestCentral::start_with_peer(target_area.path(), &origin, fixture.peer).await?;
    let artifacts = tempfile::tempdir()?;
    let result = async {
        let run = RunFixture::new(&fixture, selected, target.base_url(), artifacts.path())?;
        let source_directory = older.path.parent().ok_or("source parent missing")?;
        let before = source_files(source_directory)?;
        // Act through the compiled offline helper and actual new Central routes.
        let output = run.execute().await?;
        if !current {
            require(!output.status.success(), "stale selected file was published and the newer content archived")?;
            let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let projects = report.get("projects").and_then(serde_json::Value::as_array).ok_or("project report absent")?;
            let [project] = projects.as_slice() else { return Err("one project report required".into()); };
            require(project.get("stage").and_then(serde_json::Value::as_str)==Some("preparation"), "stale selection was not rejected at preparation")?;
            require(before == source_files(source_directory)?, "stale refusal changed or moved either source epoch")?;
            require(!run.bundle.try_exists()?, "stale source created a publishable bundle")?;
            require(target.copy_status(&older.selected.project).await.is_err(), "stale source reached the new Central import")?;
        } else {
            require(output.status.success(), &format!("current source refused: {} {}",String::from_utf8_lossy(&output.stdout),String::from_utf8_lossy(&output.stderr)))?;
            let bundle = PreparedProjectBundle::open(&run.bundle)?;
            require(bundle.manifest().source.selected.file_epoch==Epoch::new(2)? && bundle.manifest().header.manifest.tables==newer.manifest.tables, "current source did not preserve its actual complete table hashes")?;
            let mut found = false;
            for index in 0..bundle.manifest().header.pages {
                let page = bundle.page(index)?;
                if page.table=="work_trail_rows" { found |= page.rows.iter().flatten().any(|cell|matches!(cell,fleet_sync_protocol::SqlValue::Text(value) if value==UNIQUE)); }
            }
            require(found,"newer unique record was not exported")?;
            require(before == source_files(&bundle.manifest().backup_directory)?, "whole original directory was not preserved intact")?;
            require(!source_directory.try_exists()?, "confirmed current source was not archived")?;
            require(target.copy_status(&newer.selected.project).await.is_ok(), "current source did not reach real Central")?;
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let stopped = target.shutdown().await;
    result?;
    stopped?;
    Ok(())
}

#[tokio::test]
async fn persisted_supersession_refuses_before_a_writable_source_can_open() -> TestResult {
    // Arrange real project data and the SDK's persisted NotWriter authority observation.
    let fixture = CutoverHostFixture::new(None).map_err(boxed)?;
    let source = native_fixture(&fixture).await.map_err(boxed)?;
    let database = fixture_database(&source.path, source.manifest.database, fixture.peer)
        .await
        .map_err(boxed)?;
    database
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::NotWriter {
                current: Some(Epoch::new(2)?),
            }),
            SystemClock.now(),
        )
        .await?;
    database.close().await;
    drop(database);
    let retained =
        fleet_sync_writer::RetainedFile::open_read_only(&source.path, BoardProjectSchema::new()?)
            .await?;
    require(
        matches!(
            retained.authority(),
            Some(RetainedAuthority::Superseded { .. })
        ),
        "SDK did not persist the authority loss",
    )?;
    drop(retained);
    let before = source_files(source.path.parent().ok_or("source directory missing")?)?;
    let mut host = fixture.acquire().map_err(boxed)?;
    // Act with a second retained reader held: a writable reopen would take the five-second admission path.
    let retained =
        fleet_sync_writer::RetainedFile::open_read_only(&source.path, BoardProjectSchema::new()?)
            .await?;
    let outcome = NativeProjectSnapshot::open(&mut host, source.selected).await;
    let error = match outcome {
        Err(error) => error,
        Ok(source) => {
            source.close().await?;
            return Err("superseded source was accepted".into());
        }
    };
    // Assert source selection itself refuses, rather than attempting a writable owner.
    require(
        matches!(
            error,
            board_staging_cutover::NativeSourceError::AuthorityLost
        ),
        "superseded source reached writable admission",
    )?;
    require(
        before == source_files(source.path.parent().ok_or("source directory missing")?)?,
        "superseded refusal changed source files",
    )?;
    drop(retained);
    Ok(())
}

fn boxed(error: impl std::fmt::Display) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(error.to_string()).into()
}
fn origin(fixture: &CutoverHostFixture) -> TestResult<ActorOrigin> {
    Ok(ActorOrigin {
        installation: String::from(fixture.service.clone()).try_into()?,
        machine: "current native selection regression"
            .to_owned()
            .try_into()?,
    })
}

async fn newer_writer(
    fixture: &CutoverHostFixture,
    source: &NativeFixture,
    central: &TestCentral,
) -> TestResult<NativeFixture> {
    let mut host = fixture.acquire().map_err(boxed)?;
    let mut snapshot = NativeProjectSnapshot::open(&mut host, source.selected.clone()).await?;
    let manifest = snapshot.preflight().manifest.clone();
    let mut pages = Vec::new();
    while let Some(page) = snapshot.next_page().await? {
        pages.push(page.contents);
    }
    snapshot.close().await?;
    drop(host);
    central
        .import_board_snapshot(pages.clone(), manifest.clone())
        .await?;
    let copy = central.copy_status(&source.selected.project).await?;
    let inputs = central.session_inputs()?;
    let receiver = inputs.receiver;
    let session_area = tempfile::tempdir()?;
    let session = ClaimSession::start(session_area.path(), inputs).await?;
    let grant = session.claim(manifest.database).await;
    let stopped = session.shutdown().await;
    let grant = grant?;
    stopped?;
    let path = source.path.with_file_name("e2.sqlite");
    let database = Arc::new(
        ReplicatedDatabase::seeding(&path, BoardProjectSchema::new()?, grant, BOARD_LEASE_POLICY)
            .await?,
    );
    let staging_area = tempfile::tempdir()?;
    let staging = StagingStore::open(staging_area.path()).await?;
    let install = SnapshotInstall {
        install_id: InstallId::new(uuid::Uuid::now_v7())?,
        database: manifest.database,
        position: manifest.position,
        schema: manifest.schema.clone(),
    };
    staging.prepare(&install, &copy).await?;
    let mut cursor = None;
    for (index, page) in pages.into_iter().enumerate() {
        staging
            .stage_page(&install, u32::try_from(index)?, cursor.as_ref(), &page)
            .await?;
        cursor = page.next;
    }
    staging.verify(&install, &manifest).await?;
    staging.publish_writer(&install, &database).await?;
    database.mark_bootstrap_ready(&install, receiver).await?;
    database.migrate().await?;
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("verified seed receipt missing")?;
    let journal = database
        .journal_store(Arc::new(fleet_sync_tokio_runner::TokioHost::new(
            Arc::new(NoCalls),
            SystemClock,
        )))
        .await?;
    journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                message_board_storage::record_snapshot_capture(scope, &receipt)
                    .await
                    .map_err(durable_steps::StoreError::backend)
            })
        })
        .await?;
    drop(journal);
    let store =
        ProjectStore::from_database(source.selected.project.clone(), database.clone()).await?;
    let mut write = store.begin_admission(origin(fixture)?).await?;
    let Some(Record::Project(state)) = write
        .record(&source.selected.project.clone().into())
        .await?
    else {
        return Err("source project missing".into());
    };
    let row_id = WorkTrailRowId::generate();
    let id: RecordId = row_id.clone().into();
    let row = WorkTrailRow {
        envelope: message_board::RecordEnvelope {
            id,
            request: Some(RequestId::generate()),
            ..state.envelope
        },
        row_id,
        project_id: source.selected.project.clone(),
        subject: TrailSubject::Project,
        text: UNIQUE.to_owned().try_into()?,
        tags: vec![],
        links: vec![],
        redacted: false,
    };
    let current = write.append_record(Record::WorkTrailRow(row)).await?;
    write.commit().await?;
    drop(store);
    let mut snapshot = database.snapshot().await?;
    let manifest = snapshot.manifest().await?;
    snapshot.close().await?;
    database.close().await;
    drop(database);
    Ok(NativeFixture {
        selected: board_staging_cutover::SelectedWriterFile {
            file_epoch: Epoch::new(2)?,
            ..source.selected.clone()
        },
        manifest,
        record_time: source.record_time,
        cursor: current,
        path,
        scope: source.scope.clone(),
    })
}

struct NoCalls;
impl fleet_sync_tokio_runner::CallRouter for NoCalls {
    async fn dispatch(
        &self,
        _: fleet_sync_tokio_runner::CallEnvelope,
    ) -> Result<serde_json::Value, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::NotSent {
            reason: durable_steps::NotSentReason::Overloaded,
        })
    }
}

fn source_files(
    root: &std::path::Path,
) -> TestResult<std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>>> {
    let mut result = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_owned()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let relative = path.strip_prefix(root)?.to_owned();
            let metadata = std::fs::symlink_metadata(&path)?;
            if metadata.is_dir() {
                result.insert(relative, None);
                pending.push(path);
            } else if metadata.is_file() {
                result.insert(relative, Some(std::fs::read(path)?));
            } else {
                return Err("unexpected native fixture entry".into());
            }
        }
    }
    Ok(result)
}
