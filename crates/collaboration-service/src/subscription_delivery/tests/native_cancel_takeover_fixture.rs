//! Native snapshot publication and persisted stale authority, with registry policy as typed input.
use super::*;
use board_fleet_sync::{BOARD_LEASE_POLICY, BoardProjectSchema};
use fleet_sync_protocol::{
    CopyRole, CopyStatus, Epoch, InstallId, LeaseRefusal, LeaseState, LeaseStatus, ReceiverId,
    RenewAnswer,
};
use fleet_sync_receiver::SnapshotInstall;
use fleet_sync_tokio_runner::StagingStore;
use fleet_sync_writer::{BootstrapGrant, ReplicatedDatabase};
use message_board::MailItemRef;
use runtime_primitives::{Clock, SystemClock};

type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub(super) struct KeptProjectPrefix {
    directory: tempfile::TempDir,
    staging: StagingStore,
    install: SnapshotInstall,
}

impl KeptProjectPrefix {
    pub(super) async fn capture(fixture: &OwnerFixture) -> Result<Self, FixtureError> {
        let source = fixture.registry.database(&fixture.project).await?;
        let mut snapshot = source.snapshot().await?;
        let install = SnapshotInstall {
            install_id: InstallId::new(uuid::Uuid::now_v7())?,
            database: source.database(),
            position: snapshot.position(),
            schema: snapshot.schema().clone(),
        };
        let directory = tempfile::tempdir()?;
        let staging = StagingStore::open(directory.path().join("accepted-prefix")).await?;
        staging
            .prepare(
                &install,
                &CopyStatus {
                    database: install.database,
                    role: CopyRole::Authoritative,
                    fence: install.position.epoch,
                    holder: Some(PeerId::try_from(
                        "0192a1b2-c3d4-7000-8000-000000000001".to_owned(),
                    )?),
                    start: fleet_sync_protocol::ReplicationPosition::new(
                        install.position.epoch,
                        fleet_sync_protocol::CommitSequence::new(0),
                    ),
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
        Ok(Self {
            directory,
            staging,
            install,
        })
    }

    // The native seed and capture baseline are real; grant/copy acceptance is fixture policy.
    pub(super) async fn publish_successor(
        self,
        target: &OwnerFixture,
        project: &message_board::ProjectId,
    ) -> Result<SnapshotInstall, FixtureError> {
        let epoch = Epoch::new(
            self.install
                .position
                .epoch
                .value()
                .checked_add(1)
                .ok_or("epoch overflow")?,
        )?;
        let peer = PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned())?;
        let directory = target
            .directory
            .path()
            .join("projects")
            .join(project.as_str());
        tokio::fs::create_dir_all(&directory).await?;
        let now = SystemClock.now();
        let successor = Arc::new(
            ReplicatedDatabase::seeding(
                &directory.join(format!("e{}.sqlite", epoch.value())),
                BoardProjectSchema::new()?,
                BootstrapGrant {
                    lease: LeaseStatus {
                        database: self.install.database,
                        epoch,
                        holder: peer,
                        state: LeaseState::Held,
                        expires_at: now.after(BOARD_LEASE_POLICY.lease_duration),
                        remaining_ms: 14_400_000,
                    },
                    request_started: now,
                    start: self.install.position,
                },
                BOARD_LEASE_POLICY,
            )
            .await?,
        );
        self.staging
            .publish_writer(&self.install, &successor)
            .await?;
        successor
            .mark_bootstrap_ready(&self.install, ReceiverId::new(uuid::Uuid::now_v7())?)
            .await?;
        successor.migrate().await?;
        let receipt = successor
            .installed_snapshot()
            .await?
            .ok_or("successor seed receipt")?;
        assert!(receipt.verified);
        assert_eq!(receipt.install, self.install);
        let host = Arc::new(fleet_sync_tokio_runner::TokioHost::new(
            Arc::new(NoSeedCalls),
            SystemClock,
        ));
        let journal = successor.journal_store(host).await?;
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
        successor.close().await;
        Ok(self.install)
    }

    pub(super) async fn discard_later_group(
        self,
        fixture: &OwnerFixture,
        group: &MailItemRef,
        target: &MailItemRef,
    ) -> Result<(), FixtureError> {
        let epoch = Epoch::new(
            self.install
                .position
                .epoch
                .value()
                .checked_add(1)
                .ok_or("epoch overflow")?,
        )?;
        let peer = PeerId::new(uuid::Uuid::now_v7())?;
        let now = SystemClock.now();
        let successor = Arc::new(
            ReplicatedDatabase::seeding(
                &self
                    .directory
                    .path()
                    .join(format!("e{}.sqlite", epoch.value())),
                BoardProjectSchema::new()?,
                BootstrapGrant {
                    lease: LeaseStatus {
                        database: self.install.database,
                        epoch,
                        holder: peer,
                        state: LeaseState::Held,
                        expires_at: now.after(BOARD_LEASE_POLICY.lease_duration),
                        remaining_ms: 14_400_000,
                    },
                    request_started: now,
                    start: self.install.position,
                },
                BOARD_LEASE_POLICY,
            )
            .await?,
        );
        self.staging
            .publish_writer(&self.install, &successor)
            .await?;
        successor
            .mark_bootstrap_ready(&self.install, ReceiverId::new(uuid::Uuid::now_v7())?)
            .await?;
        successor.migrate().await?;
        let receipt = successor
            .installed_snapshot()
            .await?
            .ok_or("successor install receipt")?;
        assert!(receipt.verified);
        assert_eq!(receipt.install, self.install);
        let successor_store = message_board_storage::ProjectStore::from_database(
            fixture.project.clone(),
            successor.clone(),
        )
        .await?;
        let mut read = successor_store.snapshot().await?;
        assert!(
            read.mail_handoff(target).await?.is_some(),
            "the kept prefix omitted the original frozen follow"
        );
        assert!(
            read.mail_handoff(group).await?.is_none(),
            "the discarded group survived in the new epoch file"
        );
        assert!(
            read.mail_cancel_expansion_for_target(group, target)
                .await?
                .is_none(),
            "a discarded child association survived"
        );
        read.finish().await?;
        let old = fixture.registry.database(&fixture.project).await?;
        // The real FleetService takeover is an integration obligation. Here its
        // typed stale-epoch answer exercises the actual persisted native fence.
        old.observe_renewal(
            RenewAnswer::Refused(LeaseRefusal::StaleEpoch {
                current_epoch: epoch,
                epoch_ended_at: self.install.position,
            }),
            SystemClock.now(),
        )
        .await?;
        assert!(
            matches!(old.writer_state().await?, fleet_sync_lease::WriterState::Superseded { current } if current == epoch)
        );
        fixture.registry.supersede_project(&fixture.project).await?;
        assert_eq!(
            fixture.registry.superseded_current(&fixture.project).await,
            Some(Some(message_board::EpochNumber::try_from(epoch.value())?))
        );
        successor_store.close().await;
        drop(successor);
        Ok(())
    }
}

struct NoSeedCalls;
impl fleet_sync_tokio_runner::CallRouter for NoSeedCalls {
    async fn dispatch(
        &self,
        _: fleet_sync_tokio_runner::CallEnvelope,
    ) -> Result<serde_json::Value, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::AnswerLost)
    }
}
