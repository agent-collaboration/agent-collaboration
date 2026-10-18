//! Containment must preserve healthy subscription rows and report the refused project.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::keyed_call;
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn a_contained_project_is_reported_beside_exact_healthy_subscription_rows()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_poll().await?;
    let (healthy, _) = fixture.project().await?;
    let (corrupt, _) = fixture.project().await?;
    let reader: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"listed reader"}))?;
    for project in [&healthy, &corrupt] {
        accepted(&keyed_call(&fixture,"thread_subscribe",json!({"actor":reader,"scope":{"kind":"project","projectId":project},"policy":{"mode":"poll"}})).await?);
    }
    let request = json!({"actor":reader});
    let before = fixture
        .api
        .call("thread_subscriptions", request.clone())
        .await?;
    let rows = before
        .pointer("/result/subscriptions")
        .and_then(Value::as_array)
        .ok_or("initial subscription rows")?;
    assert_eq!(rows.len(), 2, "{before}");
    let expected: Vec<_> = rows
        .iter()
        .filter(|row| row.get("projectId") == Some(&json!(healthy)))
        .cloned()
        .collect();
    assert_eq!(expected.len(), 1);
    let store = fixture
        ._registry
        .existing_project(&corrupt)
        .await?
        .ok_or("corrupt project")?;
    let scope = SubscriptionScope::project(corrupt.clone());
    let mut read = store.snapshot().await?;
    let membership = read
        .membership(&reader, &scope)
        .await?
        .ok_or("membership")?;
    read.finish().await?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    assert_eq!(sqlx::query("UPDATE record_identities SET payload=json_set(payload,'$.record.policy.mode','invalid-mode') WHERE kind='subscriptionMembership' AND id=?").bind(membership.envelope.id.as_str()).execute(&mut raw).await?.rows_affected(),1);
    raw.close().await?;
    for _ in 0..2 {
        let listed = fixture
            .api
            .call("thread_subscriptions", request.clone())
            .await?;
        assert_eq!(
            listed.pointer("/result/subscriptions"),
            Some(&json!(expected)),
            "{listed}"
        );
        assert_eq!(
            listed.pointer("/result/unavailable"),
            Some(&json!([corrupt.clone()])),
            "{listed}"
        );
    }
    let mut read = store.snapshot().await?;
    assert!(
        read.membership(&reader, &scope).await.is_err(),
        "direct membership inspection must still refuse corruption"
    );
    read.finish().await?;
    assert!(
        fixture._registry.existing_project(&corrupt).await.is_err(),
        "definitive failure stays quarantined"
    );
    drop(store);
    fixture.stop().await
}
