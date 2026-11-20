use super::*;
use collaboration_protocol::{
    ConversationAdmissionState, ProviderOperationKind, ProviderOperationStage,
};
use std::{task::Poll, time::Duration};

const KNOWN_UNSENT: &str =
    "the Router was busy and nothing was sent, so send again as a new operation";

async fn free_restore_rig() -> (RestoreRig, ExternalProviderSupervisor) {
    // One Host has one store lock. A separate store exercises the free path.
    let mut free = RestoreRig::new();
    free.target.session_id = SessionId::try_from("free-session".to_owned()).expect("session ID");
    free.seed_session().await;
    let host = free.open_host("restored-in-auto").await;
    (free, host)
}

#[tokio::test(flavor = "current_thread")]
async fn restore_admission_expires_with_no_row_or_owner_while_store_is_held() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;

    let (free, free_host) = free_restore_rig().await;

    let operation = OperationId::generate();
    let mut held_store = host.inner.store.lock().await;
    let admission = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    assert!(futures_util::poll!(&mut admission).is_pending());

    free.assert_loads(&free_host).await;
    rig.clock
        .advance(Duration::from_secs(11))
        .expect("advance past L");

    let failure = match futures_util::poll!(&mut admission) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("held store must not admit restore after L"),
        Poll::Pending => panic!("held store left restore admission pending past L"),
    };
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), KNOWN_UNSENT);
    assert!(
        held_store
            .inspect(&operation)
            .await
            .expect("inspect")
            .is_none()
    );
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    drop(held_store);

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("fixture shutdown");
    free_host.shutdown().await.expect("free fixture shutdown");
}

#[tokio::test(flavor = "current_thread")]
async fn restore_admission_deadline_settles_an_unacknowledged_commit_without_dispatch() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("restored-in-auto").await;
    let (free, free_host) = free_restore_rig().await;

    let mut commit = host
        .inner
        .store
        .lock()
        .await
        .pause_next_admission_after_commit_for_test();
    let operation = OperationId::generate();
    let admission = host.prepare_operation(
        host.admission_deadline(),
        operation.clone(),
        ProviderOperationKind::ConversationLoad,
        binding(),
        Some(&rig.target),
    );
    tokio::pin!(admission);
    tokio::select! {
        () = commit.reached() => {},
        _ = &mut admission => panic!("admission returned before the commit witness"),
    }

    free.assert_loads(&free_host).await;
    rig.clock
        .advance(Duration::from_secs(10))
        .expect("advance to L");
    let failure = match futures_util::poll!(&mut admission) {
        Poll::Ready(Err(failure)) => failure,
        Poll::Ready(Ok(_)) => panic!("restore admitted at L with an unacknowledged commit"),
        Poll::Pending => panic!("unacknowledged commit left restore admission pending at L"),
    };
    assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert_eq!(String::from(failure.message), KNOWN_UNSENT);
    assert!(host.owner_session_snapshot(&rig.target).is_none());

    commit.release();
    let reuse = host
        .load(rig.load_request(operation))
        .await
        .expect("reused operation after commit");
    assert_eq!(reuse.admission, ConversationAdmissionState::Existing);
    assert_eq!(reuse.operation.stage, ProviderOperationStage::Terminal);
    assert_eq!(reuse.operation.effect, ProviderOperationEffect::None);
    assert!(host.owner_session_snapshot(&rig.target).is_none());
    assert!(!rig.root.path().join("wire.json").exists());

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("fixture shutdown");
    free_host.shutdown().await.expect("free fixture shutdown");
}
