#![allow(clippy::unwrap_used, clippy::expect_used)]
mod activity_scope_proofs;
mod control_view_proofs;
mod direct_message_proofs;
mod f6_held_record_proofs;
mod flight_proofs;
mod lifecycle_proofs;
mod local_mail_proofs;
mod native_mail_attempt_proofs;
use crate::native_worker_faults;
mod owner_activity_proofs;
pub(in crate::service_identity::subscription_delivery) mod owner_fixture;
mod push_store_proofs;
mod queued_proofs;
mod restore_corruption_proofs;
mod stall_diagnosis;
pub(in crate::service_identity::subscription_delivery) mod startup_fixture;
mod startup_retry_proofs;
mod timing_proofs;
mod transient_scan_proofs;
mod wait_proofs;
use owner_fixture::*;

#[tokio::test(start_paused = true)]
async fn running_reader_stores_one_neutral_push_before_layer_zero_and_keeps_inbox_unread() {
    let before_setup = tokio::time::Instant::now();
    let fixture = OwnerFixture::new().await;
    assert_eq!(
        tokio::time::Instant::now(),
        before_setup,
        "real fixture setup advanced the paused subscription clock"
    );
    let root = fixture.root.clone();
    let runtime = fixture.native_runtime().await;
    fixture.post(&root, "PRIVATE BODY MUST NOT BE PUSHED").await;
    let request = runtime.requests.lock().await.recv().await.unwrap();
    let record = fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        record.kind,
        collaboration_protocol::PushKind::SubscriptionActivity
    );
    assert_eq!(record.body, None);
    assert_eq!(record.activity.as_ref().unwrap().ranges.len(), 1);
    assert_eq!(
        request.correlation.as_str(),
        request.payload.push_id.as_str()
    );
    assert_eq!(
        request.payload.load_policy,
        collaboration_contracts::LoadPolicy::LoadedOnly
    );
    assert!(
        request
            .payload
            .line
            .as_str()
            .starts_with("🧵 Router: new thread activity")
    );
    assert!(!request.payload.line.as_str().contains("PRIVATE"));
    assert!(!request.payload.line.as_str().contains(root.as_str()));
    runtime.await_settled().await;
    assert_eq!(fixture.unread_count().await, 1);
    runtime.close().await;
}
