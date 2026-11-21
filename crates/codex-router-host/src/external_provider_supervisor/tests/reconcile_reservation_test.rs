use super::*;

#[derive(Clone, Copy)]
enum ReconcileReadPoint {
    BeforeDispatchMark,
    AfterTerminal,
}

#[tokio::test(flavor = "current_thread")]
async fn reconcile_during_reservation_leaves_admission_unresolved_before_the_mark() {
    check_reconcile_started_during_reservation(ReconcileReadPoint::BeforeDispatchMark).await;
}

#[tokio::test(flavor = "current_thread")]
async fn reconcile_started_during_reservation_can_read_a_confirmed_terminal_row() {
    check_reconcile_started_during_reservation(ReconcileReadPoint::AfterTerminal).await;
}

async fn check_reconcile_started_during_reservation(read_point: ReconcileReadPoint) {
    let root = tempfile::tempdir().expect("temporary root");
    let runtime = ExternalProviderRuntime::initialize(
        create_fixture(),
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("fixture initializes");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("operation store opens");
    let backend = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: binding(),
            runtime,
        }],
        Arc::new(Mutex::new(store)),
    )
    .expect("supervisor initializes");
    let operation_id = OperationId::generate();

    let (entered, reserved) = tokio::sync::oneshot::channel();
    let (release_admission, release) = tokio::sync::oneshot::channel();
    *backend
        .inner
        .admission_test_pause
        .lock()
        .expect("admission pause") = Some(AdmissionTestPause { entered, release });
    let mark_hold = match read_point {
        ReconcileReadPoint::BeforeDispatchMark => {
            let (entered, marking) = tokio::sync::oneshot::channel();
            let (release_mark, release) = tokio::sync::oneshot::channel();
            *backend
                .inner
                .dispatch_mark_test_pause
                .lock()
                .expect("dispatch mark pause") = Some(AdmissionTestPause { entered, release });
            Some((marking, release_mark))
        }
        ReconcileReadPoint::AfterTerminal => None,
    };
    let create = backend.create(ConversationCreateRequest {
        work_roots: Vec::new(),
        settings: Some(collaboration_protocol::ProviderRequestedSettings {
            mode: Some("auto".into()),
            ..collaboration_protocol::ProviderRequestedSettings::default()
        }),
        operation_id: operation_id.clone(),
        endpoint: endpoint(),
        generation: Some(generation()),
        working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned())
            .expect("working directory"),
        created_by: requester().into(),
        approver: requester().into(),
        requested_policy: ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        },
    });
    tokio::pin!(create);
    tokio::select! {
        entered = reserved => entered.expect("admission reserved before reconcile"),
        _ = &mut create => panic!("create answered before the reservation was held"),
    }
    let live = backend
        .held_result_for_test(&operation_id)
        .expect("reserved live entry")
        .0;
    let reconcile = backend.reconcile(ConversationOperationReconcileRequest {
        operation_id: operation_id.clone(),
    });
    tokio::pin!(reconcile);
    assert!(futures_util::poll!(&mut reconcile).is_pending());
    release_admission.send(()).expect("release admission");
    let submission = create.await.expect("create admitted");
    assert_eq!(submission.operation.stage, ProviderOperationStage::Admitted);
    assert_eq!(submission.operation.effect, ProviderOperationEffect::None);
    let wait = || {
        backend.wait(ConversationOperationWaitRequest {
            operation_id: operation_id.clone(),
            timeout_seconds: PositiveSeconds::try_from(30).expect("wait cutoff"),
        })
    };

    match mark_hold {
        Some((marking, release_mark)) => {
            marking
                .await
                .expect("dispatch waits before taking the store");
            let before = backend
                .inner
                .store
                .lock()
                .await
                .inspect(&operation_id)
                .await
                .expect("read admission");
            let live_before = live.record();
            let reconciled = reconcile.await.expect("read admitted operation");
            assert_eq!(reconciled.stage, ProviderOperationStage::Admitted);
            assert_eq!(reconciled.effect, ProviderOperationEffect::None);
            assert_eq!(
                reconciled.reconciliation,
                ProviderReconciliationState::Unresolved
            );
            let after = backend
                .inner
                .store
                .lock()
                .await
                .inspect(&operation_id)
                .await
                .expect("read admission again");
            assert_eq!(after, before, "reconcile leaves the admitted row alone");
            let live_after = backend
                .held_result_for_test(&operation_id)
                .expect("admission remains live")
                .0;
            assert!(Arc::ptr_eq(&live, &live_after));
            assert_eq!(live_after.record(), live_before);
            assert!(live_after.result.lock().await.is_none());
            release_mark.send(()).expect("release dispatch mark");
            let settled = wait()
                .await
                .expect("the live check did not terminalize admission");
            assert_eq!(settled.operation.stage, ProviderOperationStage::Terminal);
            assert_eq!(settled.operation.effect, ProviderOperationEffect::Applied);
        }
        None => {
            let settled = wait()
                .await
                .expect("owner settles before the reconcile read");
            assert_eq!(settled.operation.stage, ProviderOperationStage::Terminal);
            assert_eq!(
                settled.operation.reconciliation,
                ProviderReconciliationState::Confirmed
            );
            let reconciled = reconcile.await.expect("read terminal operation");
            assert_eq!(reconciled.stage, ProviderOperationStage::Terminal);
            assert_eq!(reconciled.effect, ProviderOperationEffect::Applied);
            assert_eq!(
                reconciled.reconciliation,
                ProviderReconciliationState::Confirmed
            );
        }
    }
    backend.shutdown().await.expect("supervisor shuts down");
}
