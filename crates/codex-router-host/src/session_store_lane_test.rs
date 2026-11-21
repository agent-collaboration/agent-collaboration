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

pub(crate) fn mock_binding() -> ProviderBindingIdentity {
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
async fn lane_stop_releases_backlog_and_retains_a_submitted_commit() {
    use crate::provider_operation_custody::OperationLifecycle;
    use collaboration_protocol::{
        ConversationBindingIdentity, ProviderOperationKind, ProviderOperationStage,
    };
    use provider_session_machines::operation_record_machine::OperationRecord;

    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("lane-stop").await;
    let first = OperationId::generate();
    let queued = OperationId::generate();
    let first_live = Arc::new(OperationLifecycle::new(
        OperationRecord::Admitted { queued: false },
        Arc::new(Notify::new()),
    ));
    let queued_live = Arc::new(OperationLifecycle::new(
        OperationRecord::Admitted { queued: false },
        Arc::new(Notify::new()),
    ));
    let binding = Arc::new(BindingUncertainty::default());
    let lifecycles = BTreeMap::from([
        (first.clone(), first_live.clone()),
        (queued.clone(), queued_live.clone()),
    ]);
    binding.lane_custody.get_or_init(|| {
        Arc::new(move |_, operation| lifecycles.get(operation)?.begin_lane_write(Arc::new(|| {})))
    });
    let mut store = fixture.store.lock().await;
    for operation in [&first, &queued] {
        store
            .admit(ProviderOperationAdmission {
                operation_id: operation.clone(),
                operation_kind: ProviderOperationKind::ConversationPrompt,
                binding: ConversationBindingIdentity::ExternalProvider {
                    binding: fixture.binding.clone(),
                },
                admitted_at_ms: 0,
            })
            .await
            .expect("write admission");
    }
    let mut commit = store.pause_next_admission_after_commit_for_test();
    drop(store);

    let owner = owner();
    let (lane, runner) = SessionStoreLane::with_binding(
        owner,
        target,
        fixture.store.clone(),
        fixture.clock,
        Arc::new(std::sync::OnceLock::from(binding)),
    );
    let terminal = |operation: OperationId| SessionStoreWrite::Terminal {
        operation,
        effect: ProviderOperationEffect::None,
        reconciliation: ProviderReconciliationState::Confirmed,
        stop_reason: None,
        recovery: UnknownRecovery::Blocks,
        cancelled_input: None,
    };
    let first_ticket = lane
        .offer(owner, terminal(first))
        .expect("accept active writer");
    let queued_ticket = lane
        .offer(owner, terminal(queued.clone()))
        .expect("accept backlog");
    assert!(!first_live.finish_host(CustodyEnd::NoWrite));
    assert!(!queued_live.finish_host(CustodyEnd::NoWrite));
    let task = tokio::spawn(runner.run(CancellationToken::new()));
    commit.reached().await;
    task.abort();
    assert!(task.await.expect_err("stop the lane runner").is_cancelled());

    assert!(
        !queued_live.state().custody.holds_custody(),
        "stopped lane stranded its queued ticket"
    );
    assert!(matches!(
        queued_ticket.settlement().await,
        Err(SessionStoreWaitError::Stopped)
    ));
    assert!(matches!(
        lane.offer(owner, terminal(queued.clone())),
        Err(SessionStoreOfferError::Stopped)
    ));
    assert!(
        first_live.state().custody.holds_custody(),
        "the committed write still has no receipt"
    );
    assert!(
        fixture.store.try_lock().is_err(),
        "the submitted writer still holds SQLite"
    );
    commit.release();
    assert!(matches!(
        first_ticket.settlement().await,
        Ok(SessionStoreSettlement::Operation(_))
    ));
    assert!(!first_live.state().custody.holds_custody());
    let untouched = fixture
        .store
        .lock()
        .await
        .inspect(&queued)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(untouched.stage, ProviderOperationStage::Admitted);
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_first_queue_mark_cannot_settle_an_existing_operation() {
    use collaboration_protocol::{ConversationBindingIdentity, ProviderOperationKind};

    let fixture = StoreFixture::new().await;
    let original = fixture.record_session("original").await;
    let queued = fixture.record_session("queued").await;
    let admission = ProviderOperationAdmission {
        operation_id: OperationId::generate(),
        operation_kind: ProviderOperationKind::ConversationPrompt,
        binding: ConversationBindingIdentity::ExternalProvider {
            binding: fixture.binding.clone(),
        },
        admitted_at_ms: 0,
    };
    let original_record = fixture
        .store
        .lock()
        .await
        .admit_queued_prompt_at_dispatch(admission.clone(), &original, 0)
        .await
        .expect("original operation written");
    let operation = admission.operation_id.clone();
    let owner = owner();
    let (lane, runner) =
        SessionStoreLane::new(owner, queued, fixture.store.clone(), fixture.clock.clone());
    lane.defer_queued_admission(owner, admission)
        .expect("held in memory");
    let mark = lane
        .offer(
            owner,
            SessionStoreWrite::MarkDispatch(MarkedInput {
                operation: operation.clone(),
                input: InputId::generate(),
            }),
        )
        .expect("first mark accepted");
    let terminal = lane
        .offer(
            owner,
            SessionStoreWrite::Terminal {
                operation: operation.clone(),
                effect: ProviderOperationEffect::None,
                reconciliation: ProviderReconciliationState::Confirmed,
                stop_reason: None,
                recovery: UnknownRecovery::Blocks,
                cancelled_input: None,
            },
        )
        .expect("known unsent cleanup accepted behind the mark");
    let stop = CancellationToken::new();
    let task = tokio::spawn(runner.run(stop.clone()));
    assert!(matches!(
        mark.settlement().await,
        Err(SessionStoreWaitError::Store(
            ProviderOperationStoreError::TransitionConflict
        ))
    ));
    assert!(matches!(
        terminal.settlement().await,
        Ok(SessionStoreSettlement::MarkNeverWritten)
    ));
    assert_eq!(
        fixture
            .store
            .lock()
            .await
            .inspect(&operation)
            .await
            .expect("row inspection"),
        Some(original_record)
    );
    stop.cancel();
    task.await.expect("lane stopped");
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_terminal_offer_blocks_the_next_owner_even_after_its_write_fails() {
    let fixture = StoreFixture::new().await;
    let a = fixture.record_session("a").await;
    let b = fixture.record_session("b").await;
    let (old, successor, b_owner) = (owner(), owner(), owner());
    let permit = Arc::new(BindingUncertainty::default());
    assert!(permit.outcomes.set(Mutex::new(Default::default())).is_ok());
    let binding = Arc::new(std::sync::OnceLock::from(permit));
    let (a_lane, a_runner) = SessionStoreLane::with_binding(
        old,
        a,
        fixture.store.clone(),
        fixture.clock.clone(),
        binding.clone(),
    );
    let (b_lane, _b_runner) = SessionStoreLane::with_binding(
        b_owner,
        b,
        fixture.store.clone(),
        fixture.clock.clone(),
        binding,
    );
    let mark = || MarkedInput {
        input: InputId::new(Uuid::now_v7().to_string()).expect("fixture input"),
        operation: OperationId::generate(),
    };

    let held = fixture.store.lock().await;
    let terminal = a_lane
        .offer(
            old,
            SessionStoreWrite::Terminal {
                operation: OperationId::generate(),
                effect: ProviderOperationEffect::Unknown,
                reconciliation: ProviderReconciliationState::Unresolved,
                stop_reason: None,
                recovery: UnknownRecovery::Blocks,
                cancelled_input: None,
            },
        )
        .expect("accept terminal while persistence is held");
    let handover = a_lane
        .handover(old, successor, Vec::new())
        .expect("handover");
    assert!(matches!(
        a_lane.offer(successor, SessionStoreWrite::MarkDispatch(mark())),
        Err(SessionStoreOfferError::EarlierOutcomeUnknown { .. })
    ));
    assert!(a_lane.accepted_marks(successor).is_empty());
    assert!(
        b_lane
            .offer(b_owner, SessionStoreWrite::MarkDispatch(mark()))
            .is_ok()
    );

    drop(held);
    let stop = CancellationToken::new();
    let run = tokio::spawn(a_runner.run(stop.clone()));
    assert!(matches!(
        terminal.settlement().await,
        Err(SessionStoreWaitError::Store(
            ProviderOperationStoreError::NotFound
        ))
    ));
    handover
        .barrier
        .settlement()
        .await
        .expect("handover drained");
    assert!(matches!(
        a_lane.offer(successor, SessionStoreWrite::MarkDispatch(mark())),
        Err(SessionStoreOfferError::EarlierOutcomeUnknown { .. })
    ));
    stop.cancel();
    run.await.expect("lane stopped");
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
