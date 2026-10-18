use super::*;
use sqlx::{ConnectOptions, Connection};

async fn corrupt(fixture: &SubscriptionFixture, statement: &'static str) -> Result<(), TestError> {
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    sqlx::query(statement).execute(&mut raw).await?;
    raw.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn corrupted_subscription_enum_fails_closed_with_field_name() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::joined().await?;
    corrupt(&fixture,"UPDATE record_identities SET payload=json_set(payload,'$.record.policy.mode','unknown-mode') WHERE kind='subscriptionMembership'").await?;
    let error = fixture
        .subscription(
            &fixture.reader,
            &SubscriptionScope::thread(fixture.root.clone()),
            fixture.now,
        )
        .await
        .expect_err("invalid mode was accepted");
    assert!(error.to_string().contains("mode"), "{error}");
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn expiration_does_not_normalize_a_corrupt_subscription_row() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let reader = fixture.reader.clone();
    corrupt(&fixture,"UPDATE record_identities SET payload=json_set(payload,'$.record.policy.mode','unknown-mode','$.record.expiresAt','2000-01-01T00:00:00.000Z') WHERE kind='subscriptionMembership'").await?;
    let error = fixture
        .rescan(&reader, fixture.now)
        .await
        .expect_err("corrupt expired membership was accepted");
    assert!(error.to_string().contains("mode"), "{error}");
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    let stored: String = sqlx::query_scalar(
        "SELECT payload FROM record_identities WHERE kind='subscriptionMembership'",
    )
    .fetch_one(&mut raw)
    .await?;
    let stored: serde_json::Value = serde_json::from_str(&stored)?;
    assert_eq!(
        stored.pointer("/record/policy/mode"),
        Some(&serde_json::json!("unknown-mode"))
    );
    assert_eq!(
        stored.pointer("/record/ended"),
        Some(&serde_json::Value::Null)
    );
    raw.close().await?;
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn m2_corrupt_ended_row_reports_the_missing_ended_at_field() -> Result<(), TestError> {
    let mut fixture = SubscriptionFixture::joined().await?;
    let scope = SubscriptionScope::thread(fixture.root.clone());
    fixture
        .admit(
            fixture.reader.clone(),
            BoardOperation::ThreadUnsubscribe {
                scope: scope.clone(),
            },
            fixture.now,
        )
        .await?;
    corrupt(&fixture,"UPDATE record_identities SET payload=json_remove(payload,'$.record.ended.at') WHERE kind='subscriptionMembership'").await?;
    let error = fixture
        .subscription(&fixture.reader, &scope, fixture.now)
        .await
        .expect_err("missing ended at was accepted");
    let error = error
        .downcast_ref::<BoardError>()
        .ok_or("wrong error family")?;
    assert!(
        matches!(&error.details,BoardErrorDetails::FieldConstraint {field,..} if field=="subscriptionMembership.ended.at"),
        "{error:?}"
    );
    fixture.stop().await
}
