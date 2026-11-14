use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "An actual SQLite trigger failure must prevent an acknowledgement and preserve the complete retained floor."
)]
#[tokio::test]
async fn floor_write_failure_is_unavailable_unchanged_and_a_healthy_retry_acknowledges_commit()
-> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let before = f
        .rows(
            A,
            "SELECT * FROM organization_authorization_floors ORDER BY id",
            vec![],
        )
        .await?;
    f.apply(A,"CREATE TRIGGER signin_floor_failure BEFORE UPDATE ON organization_authorization_floors BEGIN SELECT RAISE(ABORT,'fixture authorization floor failure'); END",vec![]).await?;
    let incoming = organization_floor(A, 4, "stopped");
    let failed = f.post(REVISIONS, &incoming).await?;
    assert_eq!(failed.status, 503);
    assert!(
        failed.body.is_empty(),
        "no failed floor write publishes an acknowledgement"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT * FROM organization_authorization_floors ORDER BY id",
            vec![]
        )
        .await?,
        before,
        "the failed update must leave every retained floor column unchanged"
    );
    f.apply(A, "DROP TRIGGER signin_floor_failure", vec![])
        .await?;
    let retried = f.post(REVISIONS, &incoming).await?;
    assert_eq!(retried.status, 200);
    assert_eq!(retried.value()?, json!({"acknowledgedRevision":4}));
    assert_eq!(
        f.rows(
            A,
            "SELECT minimum_revision,state FROM organization_authorization_floors",
            vec![]
        )
        .await?,
        vec![vec![number(4)?, text("stopped")]]
    );
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 3, "active"))
            .await?
            .value()?,
        json!({"acknowledgedRevision":4}),
        "older push receives the committed higher floor"
    );
    f.close().await
}
