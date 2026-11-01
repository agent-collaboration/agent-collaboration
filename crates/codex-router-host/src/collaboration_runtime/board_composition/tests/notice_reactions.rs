//! Production reaction owner, real project runtimes and session reads; central notice answers and Fleet remain named stand-ins.
use super::super::machine_notice_reactions::MachineNoticeReactions;
use super::*;
use board_fleet_sync::MachineNoticeProgressStore;
use board_sync_protocol::{BoardConsumerProtocol, BoardNotice, NoticeRead, PositionedBoardNotice};
use fleet_sync_protocol::{Epoch, NoticePosition};
use fleet_sync_tokio_runner::SessionRequests;
use message_board_storage::{FleetCache, FleetRecordSnapshot};
use std::os::unix::fs::PermissionsExt;

#[path = "archive_sync_barrier.rs"]
mod archive_sync_barrier;

#[cfg(feature = "runtime-test-support")]
#[path = "central_read_copies.rs"]
mod central_read_copies;
#[path = "notice_authority.rs"]
mod hint_authority;
#[cfg(feature = "runtime-test-support")]
#[path = "persisted_supersession.rs"]
mod persisted_supersession;

struct RenewalPromptReceipts {
    receiver: tokio::sync::broadcast::Receiver<(ProjectId, runtime_primitives::OperationId)>,
    operations: std::collections::BTreeSet<runtime_primitives::OperationId>,
}

struct Runtime {
    directory: tempfile::TempDir,
    central: TestFleet,
    admission: Arc<ProjectAdmission>,
    fleet: Arc<FleetClient>,
    projects: Arc<super::super::project_writers::ProjectWriters>,
    requests: SessionRequests<BoardConsumerProtocol>,
    running: super::super::machine_session::RunningMachineSession,
    #[cfg(feature = "runtime-test-support")]
    readiness: watch::Receiver<bool>,
    project: ProjectId,
    epoch: Epoch,
    renewal_receipts: std::sync::Mutex<RenewalPromptReceipts>,
}

impl Runtime {
    async fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let directory = tempfile::tempdir()?;
        let root = directory.path().join("router");
        let mut central =
            TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
        let board = open_board_admission(&root, identity()?, Some(central.inputs()?), None).await?;
        let session = board.session.as_ref().ok_or("session missing")?;
        let projects = session.projects.clone();
        let requests = session.requests.clone();
        let ready = session.welcomed.subscribe();
        #[cfg(feature = "runtime-test-support")]
        let readiness = ready.clone();
        let (fleet, creation) = arrange_project(&board, &root).await?;
        let admission = board.admission.clone();
        // Prepare the local directory before starting the bounded handshake.
        // Holding authentication while doing SQLite setup can outlive the real
        // transport's connection deadline and close the fixture's waiter.
        settle_fixture_directory(&fleet, &creation).await?;
        let cache = FleetCache::open(&root.join("fleet-cache.sqlite")).await?;
        central
            .freeze_fleet_snapshot(FleetRecordSnapshot {
                through: cache.checkpoint().await?,
                records: fleet.records().await?,
            })
            .await?;
        cache.close().await;
        assert_eq!(
            central
                .auth
                .upgrades
                .load(std::sync::atomic::Ordering::SeqCst),
            0,
            "local fixture inputs must be prepared before authentication starts"
        );
        let running = board.start().ok_or("session not started")?;
        timeout(BOUND, central.auth.entered.notified()).await?;
        central.release()?;
        wait_welcome(ready).await?;
        collaboration_contracts::ProjectRegistration::start(
            projects.as_ref(),
            &creation.project_id,
        )
        .await?;
        let writer = projects.ensure(&creation.project_id).await?;
        assert!(matches!(
            timeout(BOUND, writer.runtime.wait_registration()).await??,
            WriterRegistrationOutcome::Ready { .. }
        ));
        timeout(BOUND, projects.settle_operations()).await??;
        assert!(matches!(
            admission.create_project(&creation).await?.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        central.wait_notice_reads(1).await?;
        drop(writer);
        let renewal_receipts = std::sync::Mutex::new(RenewalPromptReceipts {
            receiver: projects.subscribe_renewal_prompts(),
            operations: std::collections::BTreeSet::new(),
        });
        Ok(Self {
            directory,
            central,
            admission,
            fleet,
            projects,
            requests,
            running,
            #[cfg(feature = "runtime-test-support")]
            readiness,
            project: creation.project_id,
            epoch: Epoch::new(1)?,
            renewal_receipts,
        })
    }

    fn progress_directory(&self) -> std::path::PathBuf {
        // A separate component progress owner avoids sharing a live cursor file
        // with the factory's Welcome handler. Its native action owners stay real.
        self.directory.path().join("component-notice-progress")
    }
    async fn reactions(&self) -> Result<MachineNoticeReactions, io::Error> {
        MachineNoticeReactions::open(&self.progress_directory()).await
    }
    async fn set_progress(&self, position: u64) -> TestResult {
        MachineNoticeProgressStore::open(&self.progress_directory())
            .await?
            .save(NoticePosition::new(position))
            .await?;
        Ok(())
    }
    async fn progress(&self) -> Result<NoticePosition, Box<dyn std::error::Error + Send + Sync>> {
        Ok(MachineNoticeProgressStore::open(&self.progress_directory())
            .await?
            .load()
            .await?)
    }
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract asserts; setup may fail."
    )]
    fn accepted_renewal_count(&self) -> Result<i64, Box<dyn std::error::Error + Send + Sync>> {
        // KEEP_RESULT=false removes completed renewal rows. These are receipts
        // returned by the real SDK admission, never a scripted call counter.
        let mut receipts = self
            .renewal_receipts
            .lock()
            .map_err(|_| io::Error::other("renewal observations poisoned"))?;
        loop {
            match receipts.receiver.try_recv() {
                Ok((project, operation)) => {
                    assert_eq!(project, self.project);
                    assert!(
                        receipts.operations.insert(operation),
                        "native renewal admission must return a fresh operation ID"
                    );
                }
                Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
                Err(error) => return Err(error.into()),
            }
        }
        Ok(i64::try_from(receipts.operations.len())?)
    }
    fn notice(&self, position: u64) -> PositionedBoardNotice {
        PositionedBoardNotice {
            position: NoticePosition::new(position),
            kind: BoardNotice::LeaseGranted {
                project: self.project.clone(),
                epoch: self.epoch,
            },
        }
    }
    async fn clear_cache(&self) -> TestResult {
        let cache =
            FleetCache::open(&self.directory.path().join("router/fleet-cache.sqlite")).await?;
        cache
            .replace_snapshot(&FleetRecordSnapshot {
                through: FleetPosition::ZERO,
                records: Vec::new(),
            })
            .await?;
        assert!(self.fleet.records().await?.is_empty());
        Ok(())
    }
    async fn close(self) -> TestResult {
        let Self {
            mut central,
            admission,
            fleet,
            projects,
            requests,
            running,
            directory,
            ..
        } = self;
        drop(fleet);
        drop(requests);
        running.shutdown().await?;
        Arc::try_unwrap(admission)
            .map_err(|_| io::Error::other("admission still owned"))?
            .close()
            .await?;
        drop(projects);
        central.stop().await?;
        drop(directory);
        Ok(())
    }
}

fn page(notices: Vec<PositionedBoardNotice>) -> NoticeRead {
    NoticeRead::Page {
        notices,
        as_of: chrono::Utc::now(),
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn duplicate_next_and_cold_restart_use_machine_progress_without_repeating_a_renewal()
-> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime.central.script_notice_answers(Vec::new()).await;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    reactions
        .handle_pushed_notice(
            runtime.notice(1),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(1));
    assert_eq!(runtime.accepted_renewal_count()?, before + 1);
    reactions
        .handle_pushed_notice(
            runtime.notice(1),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    drop(reactions);
    let mut reopened = runtime.reactions().await?;
    reopened
        .handle_pushed_notice(
            runtime.notice(1),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.accepted_renewal_count()?, before + 1);
    reopened
        .handle_pushed_notice(
            runtime.notice(2),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(2));
    assert_eq!(runtime.accepted_renewal_count()?, before + 2);
    assert!(runtime.central.notice_reads().await.is_empty());
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn pushed_gap_reads_after_machine_progress_and_handles_the_missing_prefix() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime
        .central
        .script_notice_answers(vec![
            page(vec![runtime.notice(1), runtime.notice(2)]),
            page(Vec::new()),
        ])
        .await;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    reactions
        .handle_pushed_notice(
            runtime.notice(3),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(3));
    assert_eq!(runtime.accepted_renewal_count()?, before + 3);
    assert_eq!(
        runtime.central.notice_reads().await,
        vec![Some(NoticePosition::new(0)), Some(NoticePosition::new(2))]
    );
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn expired_cursor_prompts_real_native_renewal_and_synchronizes_before_the_pruned_floor()
-> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime.set_progress(17).await?;
    let expected = serde_json::to_value(runtime.fleet.records().await?)?;
    assert_ne!(expected, serde_json::json!([]));
    runtime.clear_cache().await?;
    runtime
        .central
        .script_notice_answers(vec![
            NoticeRead::CursorExpired {
                floor: NoticePosition::new(40),
            },
            page(Vec::new()),
        ])
        .await;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    reactions
        .catch_up_notices(
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.accepted_renewal_count()?, before + 1);
    assert_eq!(
        serde_json::to_value(runtime.fleet.records().await?)?,
        expected
    );
    assert_eq!(runtime.progress().await?, NoticePosition::new(40));
    assert_eq!(
        runtime.central.notice_reads().await,
        vec![Some(NoticePosition::new(17)), Some(NoticePosition::new(40))]
    );
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn invalid_cursor_recovers_then_reads_without_after_from_the_retained_start() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime.set_progress(90).await?;
    let expected = serde_json::to_value(runtime.fleet.records().await?)?;
    runtime.clear_cache().await?;
    runtime
        .central
        .script_notice_answers(vec![
            NoticeRead::CursorInvalid,
            page(vec![runtime.notice(73)]),
            page(Vec::new()),
        ])
        .await;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    reactions
        .catch_up_notices(
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.accepted_renewal_count()?, before + 2);
    assert_eq!(
        serde_json::to_value(runtime.fleet.records().await?)?,
        expected
    );
    assert_eq!(runtime.progress().await?, NoticePosition::new(73));
    assert_eq!(
        runtime.central.notice_reads().await,
        vec![
            Some(NoticePosition::new(90)),
            None,
            Some(NoticePosition::new(73))
        ]
    );
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn failed_handling_does_not_save_the_position_and_retry_uses_the_same_hint() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime.set_progress(5).await?;
    let mut reactions = runtime.reactions().await?;
    let notice = PositionedBoardNotice {
        position: NoticePosition::new(6),
        kind: BoardNotice::FleetFeedAdvanced {
            through: FleetPosition::ZERO,
        },
    };
    assert!(
        reactions
            .handle_pushed_notice(
                notice.clone(),
                &runtime.projects,
                &Arc::downgrade(&runtime.admission),
                &Weak::new(),
                &Weak::new(),
                &runtime.requests
            )
            .await
            .is_err()
    );
    assert_eq!(runtime.progress().await?, NoticePosition::new(5));
    reactions
        .handle_pushed_notice(
            notice,
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(6));
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn a_noncontiguous_read_page_refuses_before_applying_or_saving_the_jump() -> TestResult {
    let runtime = Box::pin(Runtime::new()).await?;
    runtime
        .central
        .script_notice_answers(vec![page(vec![runtime.notice(3)])])
        .await;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    assert!(
        reactions
            .handle_pushed_notice(
                runtime.notice(4),
                &runtime.projects,
                &Arc::downgrade(&runtime.admission),
                &Arc::downgrade(&runtime.fleet),
                &std::sync::Weak::new(),
                &runtime.requests
            )
            .await
            .is_err()
    );
    assert_eq!(runtime.progress().await?, NoticePosition::new(0));
    assert_eq!(runtime.accepted_renewal_count()?, before);
    runtime.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn progress_write_failure_rehandles_the_hint_without_changing_lease_authority() -> TestResult
{
    let runtime = Box::pin(Runtime::new()).await?;
    runtime.set_progress(5).await?;
    let mut reactions = runtime.reactions().await?;
    let before = runtime.accepted_renewal_count()?;
    let progress_directory = runtime.progress_directory().join("machine-session");
    tokio::fs::set_permissions(&progress_directory, std::fs::Permissions::from_mode(0o500)).await?;
    let failed_save = reactions
        .handle_pushed_notice(
            runtime.notice(6),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await;
    tokio::fs::set_permissions(&progress_directory, std::fs::Permissions::from_mode(0o700)).await?;
    assert!(failed_save.is_err());
    assert_eq!(runtime.progress().await?, NoticePosition::new(5));
    assert_eq!(
        runtime.accepted_renewal_count()?,
        before + 1,
        "the native prompt happened before the failed save"
    );
    drop(reactions);
    let mut reopened = runtime.reactions().await?;
    reopened
        .handle_pushed_notice(
            runtime.notice(6),
            &runtime.projects,
            &Arc::downgrade(&runtime.admission),
            &Arc::downgrade(&runtime.fleet),
            &std::sync::Weak::new(),
            &runtime.requests,
        )
        .await?;
    assert_eq!(runtime.progress().await?, NoticePosition::new(6));
    assert_eq!(
        runtime.accepted_renewal_count()?,
        before + 2,
        "reconciliation may issue a fresh ordinary renewal"
    );
    let writer = runtime.projects.ensure(&runtime.project).await?;
    assert!(
        matches!(writer.store.writer_state().await?, fleet_sync_lease::WriterState::Writing { epoch } if epoch == runtime.epoch)
    );
    drop(writer);
    runtime.close().await
}
