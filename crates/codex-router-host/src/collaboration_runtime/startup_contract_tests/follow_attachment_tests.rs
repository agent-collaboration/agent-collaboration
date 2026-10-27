use super::{Arc, BOUND, client, registered_route_project, route_fixture, start_host, target};
use collaboration_protocol::PushKind;

#[tokio::test]
async fn host_follow_attachment_allocates_a_notice_and_retains_only_weak_owners() {
    let fixture = start_host(true).await;
    let api = client(&fixture).await;
    let reader = target(&fixture.runtime, "follow-reader");
    let actors = fixture
        .runtime
        .subscription_delivery
        .as_ref()
        .expect("Host actors")
        .clone();
    let admission = Arc::downgrade(
        fixture
            .runtime
            .project_admission
            .as_ref()
            .expect("Host admission"),
    );
    let publisher = Arc::downgrade(
        fixture
            .runtime
            .notification_publisher
            .as_ref()
            .expect("Host publisher"),
    );
    let broker = fixture
        .runtime
        .startup_test_broker
        .upgrade()
        .expect("Host broker");
    let physical = broker.interaction_store();
    Box::pin(route_fixture::post_thread_activity_for_sessions(
        &api,
        [reader.clone(), target(&fixture.runtime, "second-reader")],
        registered_route_project::route_project_requests(),
    ))
    .await;
    let record = tokio::time::timeout(BOUND, async {
        loop {
            if let Some(record) = physical
                .list_held_mail_push_records(&reader)
                .await
                .expect("physical follow notices")
                .into_iter()
                .find(|record| record.kind == PushKind::SubscriptionActivity)
            {
                break record;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the Host follow attachment must allocate a stored notice");
    assert_eq!(record.target, reader);
    assert!(
        record
            .activity
            .as_ref()
            .is_some_and(|activity| !activity.ranges.is_empty()),
        "the notice must carry the committed thread range"
    );
    assert!(
        record.origin_router_ref.is_some(),
        "the stored follow has a canonical identity"
    );
    drop(physical);
    drop(broker);
    drop(api);
    // This observer retains the reader's window-store registry. Cancel and drop the Host
    // rather than asking shutdown to unwrap that intentionally retained registry.
    drop(fixture.runtime);
    actors.shutdown().await;
    tokio::time::timeout(BOUND, async {
        while admission.upgrade().is_some() || publisher.upgrade().is_some() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the retained Host reader must release admission and publisher");
    drop(actors);
}
