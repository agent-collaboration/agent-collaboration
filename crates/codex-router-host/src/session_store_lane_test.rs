use super::*;
use collaboration_protocol::{
    CodexGeneration, EndpointId, EndpointRef, GenerationNumber, NonEmptyText, ProviderBindingId,
    ProviderBindingIdentity, ProviderCapabilities, ProviderCapability, ProviderCapabilityEvidence,
    ProviderCapabilityName, ProviderCapabilityStatus, ProviderIdentity, ProviderKind,
    ProviderRequestedPolicy, ProviderRuntimeIdentity, ProviderTransport, RouterAccess, SessionId,
    UuidIdentity,
};
use collaboration_runtime::ProviderSessionResumeInputs;
use router_clock::ManualClock;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub(crate) struct StoreFixture {
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) store: Arc<AsyncMutex<ProviderOperationStore>>,
    pub(crate) path: PathBuf,
    pub(crate) binding: ProviderBindingIdentity,
    root: tempfile::TempDir,
}

impl StoreFixture {
    pub(crate) async fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("operations.sqlite");
        let store = ProviderOperationStore::open(&path)
            .await
            .expect("real store");
        Self {
            clock: Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
            store: Arc::new(AsyncMutex::new(store)),
            path,
            binding: mock_binding(),
            root,
        }
    }

    pub(crate) async fn record_session(&self, name: &str) -> SessionRef {
        let identity: ProviderIdentity = serde_json::from_value(serde_json::json!({
            "humanId": "fixture-owner",
        }))
        .expect("identity");
        let session = SessionRef {
            endpoint: self.binding.endpoint.clone(),
            session_id: SessionId::try_from(name.to_owned()).expect("session ID"),
        };
        self.store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: session.clone(),
                working_directory: absolute_record_path(self.root.path()),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: identity.clone(),
                approver: identity,
                updated_at_ms: 0,
                resume: ProviderSessionResumeInputs {
                    requested_mode: Some("original".into()),
                    ..Default::default()
                },
            })
            .await
            .expect("record setup");
        session
    }
}

fn absolute_record_path(path: &Path) -> collaboration_protocol::ProviderWorkingDirectory {
    path.display()
        .to_string()
        .try_into()
        .expect("working directory")
}

fn mock_binding() -> ProviderBindingIdentity {
    let uuid = || UuidIdentity::try_from(Uuid::now_v7().to_string()).expect("UUID");
    ProviderBindingIdentity {
        endpoint: EndpointRef {
            service_id: uuid(),
            endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint"),
        },
        binding_id: ProviderBindingId::try_from("fixture-cursor-binding".to_owned())
            .expect("binding"),
        runtime: ProviderRuntimeIdentity {
            provider: ProviderKind::Cursor,
            runtime_name: NonEmptyText::try_from("cursor".to_owned()).expect("runtime"),
            runtime_version: None,
        },
        transport: ProviderTransport::StdioAcp,
        generation: CodexGeneration {
            service_epoch: uuid(),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        capabilities: ProviderCapabilities::try_from(vec![ProviderCapability {
            name: ProviderCapabilityName::Create,
            status: ProviderCapabilityStatus::Supported,
            evidence: ProviderCapabilityEvidence::Advertised,
        }])
        .expect("capabilities"),
    }
}

fn owner() -> OwnerIncarnation {
    OwnerIncarnation(Uuid::now_v7())
}

#[tokio::test(flavor = "current_thread")]
async fn handover_orders_a_held_confirmed_write_before_the_replacement_setup_read() {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("session").await;
    let (old, replacement) = (owner(), owner());
    let (lane, runner) =
        SessionStoreLane::new(old, target, fixture.store.clone(), fixture.clock.clone());
    let write = lane
        .offer(old, SessionStoreWrite::SaveMode("confirmed".into()))
        .expect("accepted");
    // The lane worker is held before it can acquire the shared store.
    let handover = lane
        .handover(old, replacement, Vec::new())
        .expect("insert handover");
    assert!(matches!(
        lane.offer(old, SessionStoreWrite::SaveMode("obsolete".into())),
        Err(SessionStoreOfferError::Replaced)
    ));
    let read = lane.read_setup(replacement).expect("new owner can enqueue");

    let stop = CancellationToken::new();
    let run = tokio::spawn(runner.run(stop.clone()));
    assert!(matches!(
        write.settlement().await,
        Ok(SessionStoreSettlement::ModeSaved)
    ));
    assert!(matches!(
        handover.barrier.settlement().await,
        Ok(SessionStoreSettlement::HandedOver)
    ));
    let SessionStoreSettlement::Setup(Some(setup)) =
        read.wait().await.expect("read behind barrier")
    else {
        panic!("new owner must see the saved setup");
    };
    assert_eq!(setup.resume.requested_mode.as_deref(), Some("confirmed"));
    stop.cancel();
    run.await.expect("lane drained");
}

#[tokio::test(flavor = "current_thread")]
async fn an_a_local_stall_allows_b_progress_while_a_shared_store_stall_honors_l() {
    let fixture = StoreFixture::new().await;
    let a = fixture.record_session("a").await;
    let b = fixture.record_session("b").await;
    let (a_owner, b_owner) = (owner(), owner());
    let (a_lane, a_runner) =
        SessionStoreLane::new(a_owner, a, fixture.store.clone(), fixture.clock.clone());
    let (b_lane, b_runner) = SessionStoreLane::new(
        b_owner,
        b.clone(),
        fixture.store.clone(),
        fixture.clock.clone(),
    );
    let a_held = a_lane
        .offer(a_owner, SessionStoreWrite::SaveMode("a waiting".into()))
        .expect("A accepted");
    let stop = CancellationToken::new();
    let b_task = tokio::spawn(b_runner.run(stop.clone()));

    let SessionStoreSettlement::Setup(Some(record)) = b_lane
        .read_setup(b_owner)
        .expect("B accepted")
        .wait()
        .await
        .expect("B completes while A is local")
    else {
        panic!("B needs its setup")
    };
    assert_eq!(record.target, b);
    let shared = fixture.store.lock().await;
    let b_write = b_lane
        .offer(b_owner, SessionStoreWrite::SaveMode("b late".into()))
        .expect("B write accepted");
    let b_wait = tokio::spawn(b_write.wait());
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(STORE_WAIT))
        .await;
    fixture
        .clock
        .advance(STORE_WAIT)
        .expect("advance the shared clock");
    let failed = b_wait
        .await
        .expect("B caller returned")
        .expect_err("a shared-store stall times out");
    assert!(matches!(failed, SessionStoreWaitError::Elapsed));
    assert_eq!(failed.to_string(), "the store didn't respond in time");
    drop(shared);

    // The timeout did not remove the accepted B write.
    let SessionStoreSettlement::Setup(Some(late)) = b_lane
        .read_setup(b_owner)
        .expect("later read")
        .wait()
        .await
        .expect("after the store resumes")
    else {
        panic!("B setup must remain")
    };
    assert_eq!(late.resume.requested_mode.as_deref(), Some("b late"));
    let a_task = tokio::spawn(a_runner.run(stop.clone()));
    assert!(matches!(
        a_held.settlement().await,
        Ok(SessionStoreSettlement::ModeSaved)
    ));
    stop.cancel();
    a_task.await.expect("A stopped");
    b_task.await.expect("B stopped");
}
