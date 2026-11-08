use super::*;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::AtomicStore;
use sqlx_core::executor::Executor as _;

fn corrupt_failure() -> AttemptFailure {
    AttemptFailure {
        class: ErrorClass::CorruptRow,
        message: "fleet.feed.as_of: not a timestamp".into(),
    }
}

fn minutes(count: u64) -> Duration {
    Duration::from_secs(count * 60)
}

#[test]
fn the_backoff_doubles_from_thirty_seconds_and_stops_at_ten_minutes() {
    // Arrange
    let failures = 1..=7;

    // Act
    let waits: Vec<_> = failures.map(backoff).collect();

    // Assert: 30 s, 1, 2, 4, 8 min, then capped at 10 min.
    assert_eq!(
        waits,
        [
            Duration::from_secs(30),
            minutes(1),
            minutes(2),
            minutes(4),
            minutes(8),
            minutes(10),
            minutes(10)
        ]
    );
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_first_failure_creates_one_waiting_row_due_after_thirty_seconds() -> Result<(), TestError>
{
    // Arrange
    let fixture = FleetStorageFixture::open().await?;
    let (id, subject) = (Uuid::now_v7(), Uuid::now_v7());

    // Act
    let (outcome, row, next) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let outcome = failed(
                    scope,
                    id,
                    subject,
                    AttemptKind::FeedRetention,
                    &corrupt_failure(),
                    NOW,
                )
                .await?;
                let row = attempt(scope, subject, AttemptKind::FeedRetention).await?;
                Ok((outcome, row, next_retry(scope).await?))
            })
        })
        .await?;

    // Assert
    let due = NOW.after(Duration::from_secs(30));
    assert_eq!(outcome, AttemptOutcome::Retry { at: due });
    assert_eq!(
        row,
        Some(AttemptRow {
            id,
            subject,
            kind: AttemptKind::FeedRetention,
            state: AttemptState::Waiting {
                next_attempt_at: Some(due)
            },
            attempts: 1,
            first_failed_at: Some(NOW),
            last_error: Some(corrupt_failure()),
            created_at: NOW,
            updated_at: NOW,
        })
    );
    assert_eq!(next, Some(due));
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_subject_still_failing_twenty_four_hours_after_its_first_failure_is_given_up()
-> Result<(), TestError> {
    // Arrange: a failure at NOW, then one just before and one at the 24 h mark.
    let fixture = FleetStorageFixture::open().await?;
    let subject = Uuid::now_v7();
    let record = |at: SyncTime| {
        let object = fixture.object.clone();
        async move {
            object
                .transaction(move |scope| {
                    Box::pin(async move {
                        let outcome = failed(
                            scope,
                            Uuid::now_v7(),
                            subject,
                            AttemptKind::NoticeExpiry,
                            &corrupt_failure(),
                            at,
                        )
                        .await?;
                        Ok((outcome, next_retry(scope).await?))
                    })
                })
                .await
        }
    };
    record(NOW).await?;

    // Act
    let just_before = record(NOW.after(RETIRE_AFTER - Duration::from_millis(1))).await?;
    let at_limit = record(NOW.after(RETIRE_AFTER)).await?;

    // Assert: still retried 1 ms before, given up at the limit and never due again.
    assert!(matches!(just_before.0, AttemptOutcome::Retry { .. }));
    assert_eq!(at_limit, (AttemptOutcome::GivenUp, None));
    let row = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move { attempt(scope, subject, AttemptKind::NoticeExpiry).await })
        })
        .await?
        .ok_or("given-up row vanished")?;
    assert_eq!(row.state, AttemptState::GivenUp);
    assert_eq!(row.attempts, 3);
    assert_eq!(row.first_failed_at, Some(NOW));
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn success_removes_the_row_and_its_retry() -> Result<(), TestError> {
    // Arrange
    let fixture = FleetStorageFixture::open().await?;
    let subject = Uuid::now_v7();

    // Act
    let (row, next) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                failed(
                    scope,
                    Uuid::now_v7(),
                    subject,
                    AttemptKind::PartExpiry,
                    &corrupt_failure(),
                    NOW,
                )
                .await?;
                succeeded(scope, subject, AttemptKind::PartExpiry).await?;
                Ok((
                    attempt(scope, subject, AttemptKind::PartExpiry).await?,
                    next_retry(scope).await?,
                ))
            })
        })
        .await?;

    // Assert
    assert_eq!((row, next), (None, None));
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_row_whose_state_and_columns_disagree_is_refused() -> Result<(), TestError> {
    // Arrange: rows written around the store, each breaking one rule.
    let fixture = FleetStorageFixture::open().await?;
    let cases = [
        ("in_flight", "feed_retention", "NULL", "NULL"),
        ("given_up", "feed_retention", "NULL", "1792195230000"),
        ("waiting", "not_a_kind", "NULL", "1792195230000"),
        ("paused", "feed_retention", "NULL", "1792195230000"),
    ];
    let mut refused = Vec::new();

    // Act
    for (state, kind, send_id, next) in cases {
        let subject = Uuid::now_v7();
        let id = Uuid::now_v7();
        let refusal = fixture
            .object
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute(sqlx_core::sql_str::AssertSqlSafe(format!(
                            "INSERT INTO central_work_attempts(id,subject,kind,state,send_id,next_attempt_at,attempts,first_failed_at,last_error_class,last_error,created_at,updated_at) VALUES('{id}','{subject}','{kind}','{state}',{send_id},{next},1,1792195200000,'storage','x',1792195200000,1792195200000)"
                        )))
                        .await
                        .map_err(StoreError::backend)?;
                    let read = queries::read_work_attempt(scope, subject.to_string(), kind.into())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(read.into_iter().map(decode).collect::<Vec<_>>())
                })
            })
            .await?;
        // Exactly the one row written, and it must not decode.
        refused.push(matches!(
            refusal.as_slice(),
            [Err(StoreError::Corrupt { .. })]
        ));
    }

    // Assert
    assert_eq!(refused, [true, true, true, true]);
    fixture.close().await?;
    Ok(())
}
