//! Real session and snapshot installation; directory policy remains a fixture input.
use super::*;
use collaboration_contracts::ProjectReplicationPosition;
use collaboration_service::ApplicationFixture;
use fleet_admission::{DirectoryEntry, DirectoryLease, DirectorySide, LeaseMetadata};
use fleet_sync_protocol::{CommitSequence, LeaseRefusal, RenewAnswer, ReplicationPosition};
use runtime_primitives::{Clock, SystemClock};
use std::{collections::BTreeSet, path::PathBuf};

const WIDE_REVISION: u64 = 9_007_199_254_740_993;
#[path = "central_read_copy_restart.rs"]
mod restart;
#[path = "central_read_copy_schema.rs"]
mod schema;

fn directory_entry(
    state: &ProjectState,
    epoch: u64,
) -> Result<DirectoryEntry, Box<dyn std::error::Error + Send + Sync>> {
    Ok(DirectoryEntry {
        project: state.project_id.clone(),
        name: "central read fixture".to_owned().try_into()?,
        workspace: state.envelope.scope.workspace.clone(),
        board: state.envelope.scope.board.clone().ok_or("board scope")?,
        board_archive: BoardState::Open,
        copy_side: DirectorySide::NotYetReported,
        lease_side: DirectorySide::Known(LeaseMetadata {
            lease: DirectoryLease::Held {
                epoch: epoch.try_into()?,
                holder: durable_steps::Uuid::now_v7().to_string().try_into()?,
                state: serde_json::from_value(serde_json::json!("held"))?,
                expires_at: chrono::Utc::now() + chrono::Duration::minutes(1),
            },
            lease_version: LeaseVersion::new(epoch.try_into()?, RegistryIncarnation::new(1)?, 1)?,
            as_of: "2026-10-22T10:00:00Z".parse()?,
        }),
    })
}

fn copy_files(root: &Path) -> Result<BTreeSet<PathBuf>, io::Error> {
    let mut paths = vec![root.to_owned()];
    let mut files = BTreeSet::new();
    while let Some(path) = paths.pop() {
        if !path.exists() {
            continue;
        }
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                paths.push(entry.path());
            } else if entry.file_name() == "copy.db" {
                files.insert(entry.path());
            }
        }
    }
    Ok(files)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn superseded_local_file_reads_native_central_copy_with_distinct_authority_and_pinned_generations()
-> TestResult {
    // Arrange actual board creation, writer publication and the existing machine session.
    let mut runtime = Box::pin(Runtime::new())
        .await
        .map_err(|error| io::Error::other(format!("arrange central read fixture: {error}")))?;
    let context = runtime.admission.runtime_test_context();
    let writer = runtime.projects.ensure(&runtime.project).await?;
    let mut write = writer.store.begin_admission(context.origin.clone()).await?;
    let Some(Record::Project(mut state)) = write.record(&runtime.project.clone().into()).await?
    else {
        return Err("project state missing".into());
    };
    state.envelope.request = None;
    state.revision = WIDE_REVISION.try_into()?;
    let head = write.append_record(Record::Project(state.clone())).await?;
    write.commit().await?;
    timeout(BOUND, async {
        loop {
            if runtime
                .projects
                .replicated_through(&runtime.project, head.epoch)
                .await?
                == head
            {
                return Ok::<_, BoardError>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| io::Error::other(format!("initial copy replication: {error}")))??;
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];

    // The real SDK persists loss of e1. The retained e1 file must never be read
    // again, and its epoch must not be mistaken for the directory's e9 writer.
    context
        .registry
        .database(&runtime.project)
        .await?
        .observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
                current_epoch: Epoch::new(2)?,
                epoch_ended_at: ReplicationPosition::new(Epoch::new(1)?, CommitSequence::new(0)),
            }),
            SystemClock.now(),
        )
        .await?;
    let retained = writer.store.path().to_owned();
    let source = Arc::downgrade(&writer.store);
    drop(writer);
    runtime
        .projects
        .supersede_project(&runtime.project, source)
        .await?;
    let retained_before = durable_file_hashes(&retained)?;
    let upgrades = runtime.central.auth.upgrades.load(Ordering::SeqCst);
    let application = (ApplicationFixture::new(
        "550e8400-e29b-41d4-a716-446655440000",
        "00000000-0000-4000-8000-000000000002",
    )?
    .with_project_admission(runtime.admission.clone()))
    .build();
    let view_request = ProjectViewRequest {
        project_id: runtime.project.clone(),
        include_removed: false,
    };

    // Act through the public projection and production-attached registry source.
    let view = timeout(BOUND, application.board().view_work(view_request.clone()))
        .await
        .map_err(|error| io::Error::other(format!("first central view: {error}")))??;
    assert_eq!(view.source, CopySource::ReadCopy {});
    assert_eq!(view.as_of, head);
    assert_eq!(
        view.replicated_through, head,
        "labels must use the board feed, not install position"
    );
    assert_eq!(
        view.current_writer,
        CurrentWriter::Held {
            epoch: 9.try_into()?
        }
    );
    let observation = view.writer_observation.ok_or("lease observation")?;
    assert_eq!(
        observation.lease_version,
        LeaseVersion::new(9.try_into()?, RegistryIncarnation::new(1)?, 1)?
    );
    assert_eq!(
        observation.as_of,
        "2026-10-22T10:00:00Z".parse::<chrono::DateTime<chrono::Utc>>()?
    );
    let card = timeout(
        BOUND,
        application.board().view_card(ContinuationCardRequest {
            project_id: runtime.project.clone(),
            actor: state
                .envelope
                .actor
                .declared_identity()
                .ok_or("declared card reader")?,
            subject: TrailSubject::Project,
        }),
    )
    .await
    .map_err(|error| io::Error::other(format!("central continuation card: {error}")))??;
    assert_eq!(card.source, CopySource::ReadCopy {});
    assert_eq!(card.as_of, head);
    assert_eq!(card.replicated_through, head);
    assert_eq!(card.subject, runtime.project.clone().into());
    let old_handle = context
        .registry
        .read_project(&runtime.project)
        .await?
        .ok_or("central reader")?;
    let mut pinned = old_handle.snapshot().await?;
    drop(old_handle);
    let pinned_paths = copy_files(&runtime.directory.path().join("router/board-read-cache"))?;
    assert!(!pinned_paths.is_empty(), "no real installed copy file");

    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 10)?];
    let refreshed = timeout(BOUND, application.board().view_work(view_request.clone()))
        .await
        .map_err(|error| io::Error::other(format!("refreshed central view: {error}")))??;
    assert_eq!(
        refreshed.current_writer,
        CurrentWriter::Held {
            epoch: 10.try_into()?
        }
    );
    assert_eq!(refreshed.as_of, head);
    let Some(Record::Project(old_state)) = pinned.record(&runtime.project.clone().into()).await?
    else {
        return Err("pinned project missing".into());
    };
    assert_eq!(
        old_state.revision.get(),
        WIDE_REVISION,
        "snapshot JSON rounded a native integer"
    );
    let after_refresh = copy_files(&runtime.directory.path().join("router/board-read-cache"))?;
    // Inspect the actual disposable native file read-only; the receiver installs
    // application tables without SQLx migration history or writer authority.
    {
        use sqlx::Connection as _;
        let current = after_refresh
            .difference(&pinned_paths)
            .next()
            .ok_or("new copy")?;
        let mut read = sqlx::SqliteConnection::connect_with(
            &sqlx::sqlite::SqliteConnectOptions::new()
                .filename(current)
                .read_only(true),
        )
        .await?;
        let forbidden: i64 = sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name IN ('_fleet_sync_writer','_fleet_sync_writer_engine_history','_fleet_sync_capture_bookkeeping','_sqlx_migrations','_fleet_sync_local_migrations')").fetch_one(&mut read).await?;
        assert_eq!(
            forbidden, 0,
            "central detail read constructed migration or writer state"
        );
        read.close().await?;
    }
    assert!(
        pinned_paths.intersection(&after_refresh).next().is_some(),
        "refresh removed the pinned copy file"
    );
    let pinned_copy = pinned_paths
        .intersection(&after_refresh)
        .next()
        .ok_or("pinned copy after refresh")?;
    let pinned_generation = pinned_copy
        .parent()
        .and_then(Path::parent)
        .ok_or("pinned generation directory")?;
    assert!(
        pinned_generation.is_dir(),
        "refresh removed a generation directory while its reader was open"
    );
    assert!(
        after_refresh.difference(&pinned_paths).next().is_some(),
        "refresh reused the published file"
    );
    pinned.finish().await?;
    timeout(BOUND, async {
        loop {
            if pinned_paths.iter().all(|path| !path.exists()) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| io::Error::other(format!("pinned generation disposal: {error}")))?;
    assert_eq!(
        durable_file_hashes(&retained)?,
        retained_before,
        "read reopened or changed retained writer"
    );
    assert_eq!(
        runtime.central.auth.upgrades.load(Ordering::SeqCst),
        upgrades,
        "detail reads opened another connection"
    );
    assert!(
        runtime
            .projects
            .writer_is_terminal(&runtime.project)
            .await?
    );
    assert_eq!(
        collaboration_contracts::ProjectRegistration::start(
            runtime.projects.as_ref(),
            &runtime.project
        )
        .await?,
        ProjectRegistrationState::Superseded
    );

    let before_coalescing = runtime.central.read_copy_requests.load(Ordering::SeqCst);
    let board_operations = application.board();
    let (first, second, third) = tokio::try_join!(
        board_operations.view_work(view_request.clone()),
        board_operations.view_work(view_request.clone()),
        board_operations.view_work(view_request.clone()),
    )?;
    assert_eq!(first, second);
    assert_eq!(second, third);
    assert_eq!(
        runtime.central.read_copy_requests.load(Ordering::SeqCst),
        before_coalescing + 1,
        "concurrent readers repeated the central pull"
    );

    let current = runtime.central.read_copy_entries.lock().await[0].clone();
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 9)?];
    let late = application.board().view_work(view_request.clone()).await?;
    assert_eq!(
        late.current_writer, refreshed.current_writer,
        "late directory observation regressed authority"
    );
    *runtime.central.read_copy_entries.lock().await = vec![current.clone()];
    assert_eq!(
        application
            .board()
            .view_work(view_request.clone())
            .await?
            .writer_observation,
        refreshed.writer_observation
    );
    // A conflicting same-version observation is terminal, so a subsequent
    // transient outage must not recover the generation invalidated by it.
    *runtime.central.read_copy_entries.lock().await = vec![directory_entry(&state, 10)?];
    assert!(
        application
            .board()
            .view_work(view_request.clone())
            .await
            .is_err()
    );
    runtime
        .central
        .read_copy_unavailable
        .store(true, Ordering::SeqCst);
    assert!(
        application
            .board()
            .view_work(view_request.clone())
            .await
            .is_err(),
        "transient failure reused a terminally invalidated generation"
    );
    runtime
        .central
        .read_copy_unavailable
        .store(false, Ordering::SeqCst);
    *runtime.central.read_copy_entries.lock().await = vec![current];
    // The scripted request failure may reconnect the real pump. Only a new
    // successful installation may restore a terminally invalidated generation.
    let restored = timeout(BOUND, async {
        loop {
            if let Ok(view) = application.board().view_work(view_request.clone()).await {
                return view;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|error| io::Error::other(format!("restore invalidated generation: {error}")))?;

    // A transiently unavailable source preserves the last completed generation.
    runtime.central.stop().await?;
    let stale = timeout(BOUND, application.board().view_work(view_request))
        .await
        .map_err(|error| io::Error::other(format!("offline completed generation: {error}")))??;
    assert_eq!(stale.as_of, head);
    assert_eq!(stale.current_writer, restored.current_writer);
    drop(application);
    drop(context);
    runtime.close().await
}
