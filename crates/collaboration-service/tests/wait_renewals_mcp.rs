//! Caller-keyed poll renewals use independent native ledgers and expose partial outcomes.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_service::GrantResult;
use message_board::*;
use serde_json::json;
use std::time::{Duration, Instant};
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn wait_renewals_replay_each_project_and_keep_partial_refusals_visible()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_poll().await?;
    let (first, _) = fixture.project().await?;
    let (second, _) = fixture.project().await?;
    for project in [&first, &second] {
        let mut subscribe = fixture.header();
        subscribe["observedEpoch"] = json!(1);
        subscribe["scope"] = json!({"kind":"project","projectId":project});
        subscribe["policy"] = json!({"mode":"poll","whenIdle":null,"quietSeconds":0,"capSeconds":0,"lifetimeSeconds":3600});
        accepted(&fixture.api.call("thread_subscribe", subscribe).await?);
    }
    let first_id = RequestId::generate();
    let second_id = RequestId::generate();
    let request = json!({"actor":fixture.actor,"actingFor":null,"protocolVersion":1,"filter":{"kind":"all"},"maxWaitSeconds":0,"renewals":[
        {"requestId":first_id,"observedEpoch":1,"resubmits":null,"scope":{"kind":"project","projectId":first}},
        {"requestId":second_id,"observedEpoch":1,"resubmits":null,"scope":{"kind":"project","projectId":second}}
    ]});
    fixture
        ._leases
        .set_grant(
            second.clone(),
            GrantResult::Expired {
                expired_at: "2026-10-12T00:00:00Z".parse()?,
            },
        )
        .await;
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
    fixture
        ._leases
        .set_grant(
            second.clone(),
            GrantResult::Holder {
                epoch: EpochNumber::try_from(1)?,
                deadline: Instant::now() + Duration::from_secs(60),
            },
        )
        .await;
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
