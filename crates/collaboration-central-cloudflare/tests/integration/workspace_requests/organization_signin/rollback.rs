//! A real storage trigger aborts the floor batch before any acknowledgement.
use super::support::{Fixture, REVISIONS, org_revision};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};
use serde_json::json;

async fn schema_statement(f: &Fixture, sql: &'static str) {
    let result = f
        .runtime()
        .await
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args: vec![] },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(
        matches!(result, BatchOutcome::Applied { .. }),
        "fixture trigger DDL did not apply"
    );
}

#[tokio::test]
async fn floor_write_failure_is_unavailable_unchanged_and_a_healthy_retry_acknowledges_commit() {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
    let before = f
        .read("SELECT * FROM organization_authorization_floors ORDER BY id")
        .await;
    let counts_before = f.counts().await;
    schema_statement(&f,"CREATE TRIGGER signin_floor_failure BEFORE UPDATE ON organization_authorization_floors BEGIN SELECT RAISE(ABORT,'fixture authorization floor failure'); END").await;
    let incoming = org_revision(4, "stopped");
    let failed = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .post(format!("{}{REVISIONS}", f.host.signin_origin()))
        .json(&incoming)
        .send()
        .await
        .unwrap();
    assert_eq!(
        failed
            .headers()
            .get("x-collaboration-central-entry")
            .unwrap(),
        "collaboration-central-link-entry"
    );
    assert_eq!(failed.status().as_u16(), 503);
    assert!(
        failed.bytes().await.unwrap().is_empty(),
        "a failed floor write publishes no acknowledgement or response payload"
    );
    assert_eq!(
        f.read("SELECT * FROM organization_authorization_floors ORDER BY id")
            .await,
        before,
        "every retained floor column must survive the failed batch unchanged"
    );
    assert_eq!(
        f.counts().await,
        counts_before,
        "the failed batch committed no extra record or proof"
    );
    schema_statement(&f, "DROP TRIGGER signin_floor_failure").await;
    assert_eq!(
        f.post(REVISIONS, &incoming).await,
        (200, json!({"acknowledgedRevision":4}))
    );
    assert_eq!(
        f.read("SELECT minimum_revision,state FROM organization_authorization_floors")
            .await
            .rows,
        vec![vec![
            SqlValue::Integer(4.try_into().unwrap()),
            SqlValue::Text("stopped".to_owned())
        ]]
    );
    assert_eq!(
        f.post(REVISIONS, &org_revision(3, "active")).await,
        (200, json!({"acknowledgedRevision":4})),
        "an older push receives the committed higher floor"
    );
    f.close().await;
}
