//! Missing sender custody is described from the UUIDv7 age, using the owning clock.
use super::*;
use crate::collaboration_rejection::CollaborationRejection;
use std::sync::atomic::Ordering;

async fn assert_missing_status_copy(days: i64, expected: &str) {
    // Arrange a real store and status runtime; neither owns custody for this id.
    let runtime = Runtime::new().await;
    let (mail, app) = remote_application(&runtime).await;
    let now = runtime.fixture.host.clock.load(Ordering::SeqCst);
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
    // Act/Assert first for an absent item, then for a retained sender context
    // whose real SDK Outbox answers Unknown. Both public renderers use this message.
    for has_context in [false, true] {
        if has_context {
            let id = item.clone();
            let context = collaboration_protocol::MailReadContext::Direct {
                origin: collaboration_protocol::PushOrigin::Session(caller.clone()),
                recipient: super::super::request(&runtime).target,
            };
            runtime
                .fixture
                .store
                .transaction(move |scope| {
                    Box::pin(async move {
                        interaction_storage::record_mail_read_context_in_scope(
                            scope,
                            &id,
                            &context,
                            chrono::DateTime::from_timestamp_millis(now).unwrap(),
                        )
                        .await
                        .map_err(StoreError::backend)
                    })
                })
                .await
                .unwrap();
        }
        let failure = match app.messages().message_show(request.clone()).await {
            Ok(_) => panic!("absent custody invented a retained message"),
            Err(failure) => failure,
        };
        assert_eq!(
            failure.kind,
            crate::collaboration_application::MessageFailureKind::NotFound
        );
        assert_eq!(failure.message, expected);
        let published = failure.published_rejection();
        assert_eq!(published.data.unwrap()["message"], expected);
    }
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
async fn missing_sender_status_at_day59_is_unknown() {
    assert_missing_status_copy(59, "unknown").await;
}

#[tokio::test]
async fn missing_sender_status_at_day61_says_removed_after60_days() {
    assert_missing_status_copy(61, "removed after 60 days").await;
}
