//! Real writer snapshot, staging, publication and reopen; central's grant/readiness are typed inputs.
use super::board_fixture::{BoardFixture, TestError, fixture_peer};
use fleet_sync_protocol::{
    CommitSequence, CopyRole, CopyStatus, Epoch, InstallId, LeaseState, LeaseStatus, ReceiverId,
    ReplicationPosition,
};
use fleet_sync_receiver::SnapshotInstall;
use fleet_sync_tokio_runner::StagingStore;
use fleet_sync_writer::{BootstrapGrant, ReplicatedDatabase};
use message_board::{EpochNumber, FeedCursor, ProjectId};
use runtime_primitives::{Clock, SystemClock};
use std::sync::Arc;

struct NoCalls;
impl fleet_sync_tokio_runner::CallRouter for NoCalls {
    async fn dispatch(
        &self,
        _: fleet_sync_tokio_runner::CallEnvelope,
    ) -> Result<serde_json::Value, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::AnswerLost)
    }
}

pub async fn prepare_snapshot_baseline(
    database: &Arc<ReplicatedDatabase<board_fleet_sync::BoardProjectSchema>>,
) -> Result<(), TestError> {
    let receipt = database
        .installed_snapshot()
        .await?
        .ok_or("published native snapshot")?;
    let host = Arc::new(fleet_sync_tokio_runner::TokioHost::new(
        Arc::new(NoCalls),
        SystemClock,
    ));
    let journal = database.journal_store(host).await?;
    journal
        .rule_transaction(move |scope| {
            Box::pin(async move {
                message_board_storage::record_snapshot_capture(scope, &receipt)
                    .await
                    .map_err(durable_steps::StoreError::backend)
            })
        })
        .await?;
    Ok(())
}

pub async fn bootstrap_project(
    source: &BoardFixture,
    target: &BoardFixture,
    project: &ProjectId,
) -> Result<(FeedCursor, EpochNumber), TestError> {
    bootstrap_with_readiness(source, target, project, true, true).await
}

pub async fn bootstrap_unprepared_project(
    source: &BoardFixture,
    target: &BoardFixture,
    project: &ProjectId,
) -> Result<(FeedCursor, EpochNumber), TestError> {
    bootstrap_with_readiness(source, target, project, true, false).await
}

pub async fn bootstrap_pending_project(
    source: &BoardFixture,
    target: &BoardFixture,
    project: &ProjectId,
) -> Result<(FeedCursor, EpochNumber), TestError> {
    bootstrap_with_readiness(source, target, project, false, false).await
}

async fn bootstrap_with_readiness(
    source: &BoardFixture,
    target: &BoardFixture,
    project: &ProjectId,
    ready: bool,
    baseline: bool,
) -> Result<(FeedCursor, EpochNumber), TestError> {
    let store = source
        ._registry
        .existing_project(project)
        .await?
        .ok_or("source project")?;
    // A seeded writer can have no new rows: neither its board head nor its
    // snapshot position names its actual grant. Read the admitted native epoch.
    let admission = store.begin_admission(source.record_origin()?).await?;
    let source_epoch = Epoch::new(admission.admitted_epoch()?.get())?;
    admission.rollback().await?;
    let read = store.snapshot().await?;
    let inherited = read.high_water();
    read.finish().await?;
    let source_database = source._registry.database(project).await?;
    let start = source_database
        .installed_snapshot()
        .await?
        .map(|receipt| receipt.install.position)
        .unwrap_or_else(|| ReplicationPosition::new(source_epoch, CommitSequence::new(0)));
    let mut snapshot = source_database.snapshot().await?;
    let install = SnapshotInstall {
        install_id: InstallId::new(uuid::Uuid::now_v7())?,
        database: source_database.database(),
        position: snapshot.position(),
        schema: snapshot.schema().clone(),
    };
    let staging = StagingStore::open(target._directory.path().join("snapshot-staging")).await?;
    // The fixture designates this pinned native snapshot as the accepted prefix.
    // Actual registry fencing and copy readiness remain separate integration obligations.
    staging
        .prepare(
            &install,
            &CopyStatus {
                database: install.database,
                role: CopyRole::Authoritative,
                fence: source_epoch,
                holder: Some(fixture_peer()?),
                start,
                acked: install.position,
                schema: install.schema.clone(),
            },
        )
        .await?;
    let mut cursor = None;
    let mut page = 0_u32;
    loop {
        let contents = snapshot.page(cursor.clone()).await?;
        staging
            .stage_page(&install, page, cursor.as_ref(), &contents)
            .await?;
        cursor = contents.next;
        if cursor.is_none() {
            break;
        }
        page = page.checked_add(1).ok_or("snapshot page overflow")?;
    }
    staging
        .verify(&install, &snapshot.manifest().await?)
        .await?;
    snapshot.close().await?;
    let epoch = Epoch::new(
        source_epoch
            .value()
            .checked_add(1)
            .ok_or("epoch overflow")?,
    )?;
    let directory = target
        ._directory
        .path()
        .join("projects")
        .join(project.as_str());
    tokio::fs::create_dir_all(&directory).await?;
    let now = SystemClock.now();
    let policy = board_fleet_sync::BOARD_LEASE_POLICY;
    let target_database = Arc::new(
        ReplicatedDatabase::seeding(
            &directory.join(format!("e{}.sqlite", epoch.value())),
            board_fleet_sync::BoardProjectSchema::new()?,
            BootstrapGrant {
                lease: LeaseStatus {
                    database: install.database,
                    epoch,
                    holder: fixture_peer()?,
                    state: LeaseState::Held,
                    expires_at: now.after(policy.lease_duration),
                    remaining_ms: 14_400_000,
                },
                request_started: now,
                start: install.position,
            },
            policy,
        )
        .await?,
    );
    staging.publish_writer(&install, &target_database).await?;
    if ready {
        target_database
            .mark_bootstrap_ready(&install, ReceiverId::new(uuid::Uuid::now_v7())?)
            .await?;
        target_database.migrate().await?;
        assert!(matches!(target_database.writer_state().await?,
            fleet_sync_lease::WriterState::Writing { epoch: actual } if actual == epoch));
    } else {
        assert!(matches!(target_database.writer_state().await?,
            fleet_sync_lease::WriterState::Seeding { epoch: actual } if actual == epoch));
    }
    if baseline {
        prepare_snapshot_baseline(&target_database).await?;
    }
    target_database.close().await;
    Ok((inherited, EpochNumber::try_from(epoch.value())?))
}
