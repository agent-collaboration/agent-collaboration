use super::*;
use sqlx::{ConnectOptions, Connection};

async fn corrupt(fixture: &SubscriptionFixture, statement: &'static str) -> Result<(), TestError> {
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    if sqlx::query(statement)
        .execute(&mut raw)
        .await?
        .rows_affected()
        == 0
    {
        return Err("corruption fixture changed no rows".into());
    }
    raw.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove behavior; Result propagates fixture errors.
async fn corrupted_subscription_enum_fails_closed_with_field_name() -> Result<(), TestError> {
    let fixture = SubscriptionFixture::joined().await?;
    corrupt(
        &fixture,
        "UPDATE subscription_memberships SET policy=json_set(policy,'$.mode','unknown-mode')",
    )
    .await?;
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
    corrupt(&fixture,"UPDATE subscription_memberships SET policy=json_set(policy,'$.mode','unknown-mode'),expires_at='2000-01-01T00:00:00.000Z'").await?;
    let error = fixture
        .rescan(&reader, fixture.now)
        .await
        .expect_err("corrupt expired membership was accepted");
    assert!(error.to_string().contains("mode"), "{error}");
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture.project_store().await?.path())
        .connect()
        .await?;
    let stored: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT policy,ended FROM subscription_memberships ORDER BY membership_id")
            .fetch_all(&mut raw)
            .await?;
    assert!(!stored.is_empty());
    for (policy, ended) in stored {
        let policy: serde_json::Value = serde_json::from_str(&policy)?;
        assert_eq!(
            policy.pointer("/mode"),
            Some(&serde_json::json!("unknown-mode"))
        );
        assert_eq!(ended, None);
    }
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
    corrupt(&fixture,"UPDATE subscription_memberships SET ended=json_remove(ended,'$.at') WHERE ended IS NOT NULL").await?;
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
