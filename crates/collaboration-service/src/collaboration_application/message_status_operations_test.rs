//! Missing local custody is described from the UUIDv7 age, using the owning clock.
use super::*;
use crate::collaboration_rejection::CollaborationRejection;

async fn assert_missing_status_copy(days: i64, expected: &str) {
    // Arrange a real store and status runtime; neither owns custody for this id.
    let runtime = Runtime::new().await;
    let mail = Arc::new(runtime.local_mail_runtime());
    let service = String::from(runtime.local_mail_props().machine.service_id().clone());
    let app = crate::ApplicationFixture::new(&service, &uuid::Uuid::now_v7().to_string())
        .unwrap()
        .with_interaction_store(runtime.fixture.owner.clone())
        .with_installation_peer(runtime.fixture.host.config.local.owner)
        .unwrap()
        .with_mail_delivery_runtime(mail.clone())
        .unwrap()
        .build();
    let now = runtime_primitives::Clock::now(runtime.fixture.host.clock.as_ref()).unix_millis();
    let birth = u128::try_from(now - days * 86_400_000).unwrap();
    let item: collaboration_protocol::MailItemId =
        uuid::Uuid::from_u128((birth << 80) | (7_u128 << 76) | (8_u128 << 60) | 0x59)
            .to_string()
            .try_into()
            .unwrap();
    let caller = runtime.fixture.draft().target;
    let request = collaboration_protocol::PushRecordShowParams {
        caller: caller.clone(),
        reference: item.as_str().to_owned(),
    };
    let failure = match app.messages().message_show(request).await {
        Ok(_) => panic!("absent custody invented a retained message"),
        Err(failure) => failure,
    };
    assert_eq!(
        failure.kind,
        crate::collaboration_application::MessageFailureKind::NotFound
    );
    assert_eq!(failure.message, expected);
    assert_eq!(
        failure.published_rejection().data.unwrap()["message"],
        expected
    );
    assert_eq!(
        runtime
            .sql_count("SELECT count(*) AS count FROM _fleet_sync_outbox_items")
            .await,
        0
    );
    assert_eq!(
        runtime
            .sql_count("SELECT count(*) AS count FROM router_pushes")
            .await,
        0
    );
    drop(app);
    drop(mail);
    runtime.close().await;
}

#[tokio::test]
async fn missing_local_status_at_day59_is_unknown() {
    assert_missing_status_copy(59, "unknown").await;
}

#[tokio::test]
async fn missing_local_status_at_day61_says_removed_after60_days() {
    assert_missing_status_copy(61, "removed after 60 days").await;
}
