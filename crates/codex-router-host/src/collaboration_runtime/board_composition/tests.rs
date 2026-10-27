//! Component proofs for the actual Host factory; process-level G2 remains separate.
#![allow(clippy::panic_in_result_fn)]
use super::*;
use collaboration_contracts::ProjectRegistrationState;
use collaboration_protocol::UuidIdentity;
use fleet_sync_protocol::{GateAnswer, GateReason, GateReasonCode, RegistrationRefusal};
use fleet_sync_session::SessionClientStore;
use fleet_sync_writer::WriterRegistrationOutcome;
use message_board::*;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::atomic::Ordering};
use tokio::{sync::watch, time::timeout};

#[path = "tests/archive_observation.rs"]
mod archive_observation;
#[path = "tests/archive_owner_replacement.rs"]
mod archive_owner_replacement;

#[path = "tests/attaching_server_calls.rs"]
mod attaching_server_calls;
#[cfg(feature = "runtime-test-support")]
#[path = "tests/central_directory_expiry.rs"]
mod central_directory_expiry;
#[path = "tests/claim_cli.rs"]
mod claim_cli;
#[path = "tests/claim_fixture.rs"]
mod claim_fixture;
#[path = "tests/claim_lost_grant.rs"]
mod claim_lost_grant;
#[path = "tests/claim_refusals.rs"]
mod claim_refusals;
#[path = "tests/claim_restart.rs"]
mod claim_restart;
#[path = "tests/directory_inputs.rs"]
mod directory_inputs;
#[cfg(feature = "runtime-test-support")]
#[path = "tests/forwarded_notification_fixture.rs"]
mod forwarded_notification_fixture;
#[path = "tests/forwarded_post_sender.rs"]
mod forwarded_post_sender;
#[path = "tests/importer_claim.rs"]
mod importer_claim;
#[cfg(feature = "runtime-test-support")]
#[path = "tests/messaging_rule_cases.rs"]
mod messaging_rule_cases;
#[path = "tests/notice_acknowledgement.rs"]
mod notice_acknowledgement;
#[path = "tests/notice_reactions.rs"]
mod notice_reactions;
#[path = "tests/notification_publisher.rs"]
pub(super) mod notification_publisher;
#[path = "tests/project_claim.rs"]
pub(super) mod project_claim;
#[cfg(feature = "runtime-test-support")]
#[path = "tests/project_integrity_refusal.rs"]
mod project_integrity_refusal;
#[path = "tests/received_grants.rs"]
mod received_grants;
#[cfg(feature = "runtime-test-support")]
#[path = "tests/returning_holder_expiry.rs"]
mod returning_holder_expiry;
#[path = "tests/snapshot_restart.rs"]
mod snapshot_restart;
use super::fleet_service::{BOUND, TestFleet};
#[path = "tests/project_creation_replies.rs"]
mod project_creation_replies;
use directory_inputs::{arrange_project, arrange_project_input, settle_fixture_directory};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

// No application row has replicated before the first write. This supplies that
// input at the existing acknowledgement port; native authority stays real.
struct EmptyProjectProgress;
impl collaboration_contracts::ProjectReplicationPosition for EmptyProjectProgress {
    fn acknowledged_mail_changes<'port>(
        &'port self,
        _: &'port ProjectId,
    ) -> collaboration_contracts::AcknowledgedMailChangesFuture<'port> {
        Box::pin(async { Ok(message_board_storage::AcknowledgedProjectChanges::default()) })
    }
    fn replicated_through<'port>(
        &'port self,
        _: &'port ProjectId,
        epoch: EpochNumber,
    ) -> collaboration_contracts::ReplicatedPositionFuture<'port> {
        Box::pin(async move {
            Ok(FeedCursor {
                epoch,
                position: FeedPosition::ZERO,
            })
        })
    }
}

pub(super) fn identity() -> Result<MachineIdentity, Box<dyn std::error::Error + Send + Sync>> {
    Ok(MachineIdentity::new(
        UuidIdentity::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
        Some("board session fixture"),
    )?)
}

#[tokio::test]
async fn absent_enrollment_constructs_no_machine_session_or_resume_file() -> TestResult {
    // E26 requires no session for None. A constructor that starts an anonymous
    // client or mints enrollment would create this file and fail the oracle.
    let directory = tempfile::tempdir()?;
    let board = open_board_admission(directory.path(), identity()?, None).await?;
    assert!(board.session.is_none());
    assert!(board.replication.is_none());
    assert!(!directory.path().join("machine-session").exists());
    let admission = board.admission.clone();
    drop(board);
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;
    Ok(())
}

pub(super) fn origin() -> Result<ActorOrigin, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ActorOrigin {
        installation: ServiceId::try_from("550e8400-e29b-41d4-a716-446655440000".to_owned())?,
        machine: MachineLabel::try_from("board session fixture".to_owned())?,
    })
}

pub(super) async fn wait_welcome(mut ready: watch::Receiver<bool>) -> TestResult {
    timeout(BOUND, async {
        while !*ready.borrow() {
            ready.changed().await?;
        }
        Ok::<_, watch::error::RecvError>(())
    })
    .await
    .map_err(|error| io::Error::other(format!("Host did not observe Welcome: {error}")))??;
    Ok(())
}

fn durable_file_hashes(
    path: &Path,
) -> Result<BTreeMap<String, String>, Box<dyn std::error::Error + Send + Sync>> {
    let mut hashes = BTreeMap::new();
    for candidate in [path.to_owned(), path.with_extension("sqlite-wal")] {
        if candidate.exists() {
            let name = candidate
                .file_name()
                .ok_or("native file name")?
                .to_string_lossy()
                .into_owned();
            hashes.insert(
                name,
                format!("{:x}", Sha256::digest(std::fs::read(candidate)?)),
            );
        }
    }
    Ok(hashes)
}

async fn assert_rejected_receipt_read(
    admission: &Arc<ProjectAdmission>,
    request: &ProjectCreateRequest,
) -> TestResult {
    use message_board::public_replies as public;
    let application = collaboration_service::CollaborationApplication::new(
        collaboration_service::ServiceIdentity::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone()),
    );
    let read = application
        .board()
        .request_effects(public::RequestEffectsRequest {
            scope: RequestEffectsScope::Project {
                project_id: request.project_id.clone(),
            },
            request: request.request_id.clone(),
            after: None,
        })
        .await?;
    assert_eq!(
        read,
        public::RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::UnknownRequest
        }
    );
    Ok(())
}

#[tokio::test]
async fn a_machine_session_resumes_its_identity_without_bootstrapping_a_project() -> TestResult {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut service =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    let ready = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .welcomed
        .subscribe();
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    timeout(BOUND, service.auth.entered.notified()).await?;
    assert!(!root.join("projects").exists());
    service.release()?;
    wait_welcome(ready).await?;
    let original = board_fleet_sync::MachineSessionStore::open(&root)
        .await?
        .load()
        .await?
        .ok_or("session not saved")?
        .session_id;
    assert!(!root.join("projects").exists());
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;

    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    let ready = board
        .session
        .as_ref()
        .ok_or("resumed session missing")?
        .welcomed
        .subscribe();
    let admission = board.admission.clone();
    let running = board.start().ok_or("resumed session not started")?;
    wait_welcome(ready).await?;
    let resumed = board_fleet_sync::MachineSessionStore::open(&root)
        .await?
        .load()
        .await?
        .ok_or("resumed state missing")?;
    assert_eq!(resumed.session_id, original);
    assert_eq!(service.auth.upgrades.load(Ordering::SeqCst), 2);
    assert!(!root.join("projects").exists());
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("resumed admission still owned"))?
        .close()
        .await?;
    service.stop().await?;
    Ok(())
}

#[tokio::test]
async fn accepted_directory_registration_waits_for_welcome_and_grant_before_the_first_record()
-> TestResult {
    // Directory settlement is a typed fixture input. Host bootstrap, registration,
    // the first native transaction and replication use the actual factory/runtime.
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let mut service =
        TestFleet::start(&directory.path().join("central"), GateAnswer::Admitted).await?;
    service.hold_registration().await?;
    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .projects
        .clone();
    let ready = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .welcomed
        .subscribe();
    let replication = board
        .replication
        .as_ref()
        .ok_or("progress port missing")?
        .clone();
    let (fleet, request) = arrange_project(&board, &root).await?;
    let project_directory = root.join("projects").join(request.project_id.as_str());
    assert!(
        !project_directory.exists(),
        "directory request created a native file"
    );
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    timeout(BOUND, service.auth.entered.notified()).await?;
    assert_eq!(
        settle_fixture_directory(&fleet, &request).await?,
        Some(ProjectRegistrationState::Pending)
    );
    assert!(
        !project_directory.exists(),
        "pre-Welcome bootstrap created a native file"
    );
    service.release()?;
    wait_welcome(ready).await?;
    timeout(BOUND, service.registration_gate.entered.notified()).await?;
    let writer = projects.ensure(&request.project_id).await?;
    let mut read = writer.store.snapshot().await?;
    assert_eq!(read.high_water().position, FeedPosition::ZERO);
    assert!(
        read.record(&request.project_id.clone().into())
            .await?
            .is_none()
    );
    assert!(
        read.record(&request.request_id.clone().into())
            .await?
            .is_none()
    );
    read.finish().await?;
    assert!(matches!(
        writer.store.begin_admission(origin()?).await,
        Err(message_board_storage::ProjectStorageError::WriteRefused(
            fleet_sync_writer::WriteRefusal::Provisional | fleet_sync_writer::WriteRefusal::Seeding
        ))
    ));
    let projection = collaboration_service::CollaborationApplication::new(
        collaboration_service::ServiceIdentity::new(
            "550e8400-e29b-41d4-a716-446655440000",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone())
        .with_project_replication(Arc::new(EmptyProjectProgress)),
    );
    let before_grant = projection
        .board()
        .view_work(ProjectViewRequest {
            project_id: request.project_id.clone(),
            include_removed: false,
        })
        .await;
    assert!(
        before_grant.is_err(),
        "an unregistered owner advertised a current writer: {before_grant:?}"
    );
    service.release_registration()?;
    assert!(matches!(
        timeout(BOUND, writer.runtime.wait_registration()).await??,
        WriterRegistrationOutcome::Ready { .. }
    ));
    timeout(BOUND, projects.settle_operations()).await??;
    // Native Ready must initialize without another caller retrying creation.
    let accepted = timeout(BOUND, async {
        loop {
            let mut read = writer.store.snapshot().await?;
            let ledger = read.record(&request.request_id.clone().into()).await?;
            let decision = match ledger {
                Some(Record::Request(ledger)) => Some(read.request_decision(&ledger).await?),
                None => None,
                _ => return Err("creating receipt has wrong record kind".into()),
            };
            read.finish().await?;
            if let Some(decision) = decision {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(RequestResult::Decided(
                    decision,
                ));
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    assert!(matches!(
        accepted,
        RequestResult::Decided(RequestDecision::Accepted { .. })
    ));
    let mut read = writer.store.snapshot().await?;
    let head = read.high_water();
    assert_eq!(head.position.get(), 3);
    let Some(Record::Project(state)) = read.record(&request.project_id.clone().into()).await?
    else {
        return Err("initial project state missing".into());
    };
    assert_eq!(Some(state.description), request.description);
    assert_eq!(
        state
            .envelope
            .position
            .project_cursor()
            .ok_or("project position")?
            .position
            .get(),
        1
    );
    let facts = read.records(RecordKind::Fact).await?;
    assert!(
        matches!(facts.as_slice(), [Record::Fact(Fact { kind: FactKind::Lifecycle(LifecycleFact::ProjectCreated { creating_request, .. }), .. })] if creating_request == &request.request_id)
    );
    read.finish().await?;
    assert_eq!(admission.create_project(&request).await?.result, accepted);
    let read = writer.store.snapshot().await?;
    assert_eq!(read.high_water(), head, "creating replay appended records");
    read.finish().await?;
    let view = projection
        .board()
        .view_work(ProjectViewRequest {
            project_id: request.project_id.clone(),
            include_removed: false,
        })
        .await?;
    assert_eq!(
        view.current_writer,
        CurrentWriter::Held {
            epoch: EpochNumber::INITIAL
        }
    );
    drop(projection);
    writer.sender.replicate_once(service.receiver).await?;
    assert_eq!(
        replication
            .replicated_through(&request.project_id, EpochNumber::INITIAL)
            .await?,
        head
    );
    drop(writer);
    drop(fleet);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;
    drop(projects);
    drop(replication);
    service.stop().await?;
    Ok(())
}

#[tokio::test]
async fn native_registration_refusal_moves_an_empty_file_and_replays_the_original_reason()
-> TestResult {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_test_writer()
        .try_init();
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("router");
    let reason = GateReason {
        code: GateReasonCode::new("nameConflict")?,
        detail: " Original fixture conflict detail ".to_owned(),
    };
    let mut service = TestFleet::start(
        &directory.path().join("central"),
        GateAnswer::Refused {
            reason: reason.clone(),
        },
    )
    .await?;
    service.hold_registration().await?;
    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    let projects = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .projects
        .clone();
    let requests = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .requests
        .clone();
    let ready = board
        .session
        .as_ref()
        .ok_or("session missing")?
        .welcomed
        .subscribe();
    let (fleet, request) = arrange_project(&board, &root).await?;
    let project_directory = root.join("projects").join(request.project_id.as_str());
    assert!(!project_directory.exists());
    let admission = board.admission.clone();
    let running = board.start().ok_or("session not started")?;
    timeout(BOUND, service.auth.entered.notified())
        .await
        .map_err(|error| io::Error::other(format!("Host never reached authentication: {error}")))?;
    assert_eq!(
        settle_fixture_directory(&fleet, &request).await?,
        Some(ProjectRegistrationState::Pending)
    );
    assert!(!project_directory.exists());
    service.release()?;
    wait_welcome(ready).await?;
    timeout(BOUND, service.registration_gate.entered.notified())
        .await
        .map_err(|error| {
            io::Error::other(format!("Host never reached native registration: {error}"))
        })?;
    let writer = projects.ensure(&request.project_id).await?;
    let source = writer.store.path().to_owned();
    let mut read = writer.store.snapshot().await?;
    assert_eq!(read.high_water().position, FeedPosition::ZERO);
    for kind in [RecordKind::Project, RecordKind::Fact, RecordKind::Request] {
        assert!(read.records(kind).await?.is_empty());
    }
    read.finish().await?;
    service.release_registration()?;
    assert_eq!(
        timeout(BOUND, writer.runtime.wait_registration()).await??,
        WriterRegistrationOutcome::Refused {
            reason: RegistrationRefusal::DatabaseRejected {
                reason: reason.clone()
            }
        }
    );
    timeout(BOUND, projects.settle_operations()).await??;
    assert!(!source.exists());
    let moved = source.with_extension("orphaned.sqlite");
    assert!(moved.exists());
    let expected = RequestResult::ScopeUnavailable(ScopeRefusal::ProjectRejected {
        project: request.project_id.clone(),
        reason: ProjectRejection::Registration {
            reason: ProjectRegistrationRejection {
                code: reason.code.as_str().to_owned(),
                detail: reason.detail,
            },
        },
    });
    assert_eq!(admission.create_project(&request).await?.result, expected);
    let before = durable_file_hashes(&moved)?;
    assert_rejected_receipt_read(&admission, &request).await?;
    assert_eq!(durable_file_hashes(&moved)?, before);
    // A separate caller owns its synchronization lease, so the background
    // refresh cannot make this error assertion pass through coalescing.
    let refresh = FleetClient::open(&root, projects.clone(), Some(requests)).await?;
    assert!(
        matches!(
            refresh.synchronize().await,
            Err(collaboration_runtime::FleetClientError::InvalidReply {
                field: "fleetCursor"
            })
        ),
        "the real rejected Read must remain an error after durable recovery"
    );
    refresh.close().await;
    drop(writer);
    drop(fleet);
    running.shutdown().await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("admission still owned"))?
        .close()
        .await?;
    drop(projects);
    let board = open_board_admission(&root, identity()?, Some(service.inputs()?)).await?;
    assert_eq!(
        board.admission.create_project(&request).await?.result,
        expected
    );
    let before = durable_file_hashes(&moved)?;
    assert_rejected_receipt_read(&board.admission, &request).await?;
    assert_eq!(durable_file_hashes(&moved)?, before);
    let admission = board.admission.clone();
    board
        .start()
        .ok_or("resumed session missing")?
        .shutdown()
        .await?;
    Arc::try_unwrap(admission)
        .map_err(|_| io::Error::other("resumed admission still owned"))?
        .close()
        .await?;
    service.stop().await?;
    Ok(())
}
