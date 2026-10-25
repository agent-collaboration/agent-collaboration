//! Caller-keyed poll renewals use independent native ledgers and expose partial outcomes.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/native_authority.rs"]
pub mod native_authority;
use board_fixture::{BoardFixture, ControlledClock, TestError, accepted, fixture_peer};
use message_board::*;
use runtime_primitives::{Clock, SystemClock};
use serde_json::json;
use std::{sync::Arc, time::Duration};
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn wait_renewals_replay_each_project_and_keep_partial_refusals_visible()
-> Result<(), TestError> {
    tokio::time::timeout(Duration::from_secs(30), exercise_wait_renewals()).await?
}

#[allow(clippy::panic_in_result_fn)]
async fn exercise_wait_renewals() -> Result<(), TestError> {
    let clock = Arc::new(ControlledClock::at(SystemClock.now()));
    let initial = clock.now();
    let fixture = BoardFixture::start_poll_with_clock(clock.clone()).await?;
    let (second, _) = fixture.project().await?;
    // Different genuine grant deadlines make only the second project expired.
    clock.set(initial.after(Duration::from_millis(1)));
    let (first, _) = fixture.project().await?;
    for project in [&first, &second] {
        let mut subscribe = fixture.header();
        let fields = subscribe
            .as_object_mut()
            .ok_or("subscription fixture header")?;
        fields.insert(
            "scope".to_owned(),
            json!({"kind":"project","projectId":project}),
        );
        fields.insert("policy".to_owned(), json!({"mode":"poll","whenIdle":null,"quietSeconds":0,"capSeconds":0,"lifetimeSeconds":3600}));
        accepted(&fixture.api.call("thread_subscribe", subscribe).await?);
    }
    let first_id = RequestId::generate();
    let second_id = RequestId::generate();
    let request = json!({"actor":fixture.actor,"actingFor":null,"protocolVersion":1,"filter":{"kind":"all"},"maxWaitSeconds":0,"renewals":[
        {"requestId":first_id,"scope":{"kind":"project","projectId":first}},
        {"requestId":second_id,"scope":{"kind":"project","projectId":second}}
    ]});
    let expires = initial.after(board_fleet_sync::BOARD_LEASE_POLICY.lease_duration);
    let expired = fleet_sync_protocol::LeaseStatus {
        database: fleet_sync_protocol::DatabaseId::new(uuid::Uuid::parse_str(second.as_str())?)?,
        epoch: fleet_sync_protocol::Epoch::new(1)?,
        holder: fixture_peer()?,
        state: fleet_sync_protocol::LeaseState::Held,
        expires_at: expires,
        remaining_ms: 14_400_000,
    };
    clock.set(expires);
    let partial = fixture.api.call("thread_wait", request.clone()).await?;
    assert_eq!(
        partial.pointer("/result/renewals/0/requestId"),
        Some(&json!(first_id)),
        "{partial}"
    );
    assert_eq!(
        partial.pointer("/result/renewals/0/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{partial}"
    );
    assert_eq!(
        partial.pointer("/result/renewals/1/requestId"),
        Some(&json!(second_id))
    );
    assert_eq!(
        partial.pointer("/result/renewals/1/reply/result/kind"),
        Some(&json!("leaseExpired")),
        "{partial}"
    );
    assert_eq!(
        partial.pointer("/result/outcome/kind"),
        Some(&json!("timedOut")),
        "{partial}"
    );
    let first_store = fixture
        ._registry
        .existing_project(&first)
        .await?
        .ok_or("first store")?;
    let mut read = first_store.snapshot().await?;
    let first_water = read.high_water();
    assert!(read.record(&first_id.clone().into()).await?.is_some());
    read.finish().await?;
    let second_store = fixture
        ._registry
        .existing_project(&second)
        .await?
        .ok_or("second store")?;
    let mut read = second_store.snapshot().await?;
    assert!(read.record(&second_id.clone().into()).await?.is_none());
    read.finish().await?;
    native_authority::observe_same_epoch_renewal(&fixture._registry, &second, expired, clock.now())
        .await?;
    let retried = fixture.api.call("thread_wait", request).await?;
    assert_eq!(
        retried.pointer("/result/renewals/0/reply/result"),
        partial.pointer("/result/renewals/0/reply/result"),
        "{retried}"
    );
    assert_eq!(
        retried.pointer("/result/renewals/1/reply/result/decision/kind"),
        Some(&json!("accepted")),
        "{retried}"
    );
    let read = first_store.snapshot().await?;
    assert_eq!(read.high_water(), first_water);
    read.finish().await?;
    let mut read = second_store.snapshot().await?;
    assert!(read.record(&second_id.into()).await?.is_some());
    read.finish().await?;
    fixture.stop().await?;
    Ok(())
}
