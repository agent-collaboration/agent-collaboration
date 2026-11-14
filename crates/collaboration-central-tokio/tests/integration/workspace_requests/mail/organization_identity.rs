use super::super::{Statement, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, CorruptReason, GuardedStatement, RowExpectation, StatementBatch,
    StoreFailure,
};
use collaboration_central_tokio::{ObjectStorage, TokioWorkspaceHost};
use collaboration_protocol::central_mail::{
    CentralMailAnswer as Answer, CentralMailOperation as Operation, CentralMailRequest,
    CentralMailResult as Result,
};
use router_clock::RuntimeClock;
use std::sync::Arc;

async fn set_wake(
    fixture: &Fixture,
    request_id: u32,
) -> (workspace_ids::WakeId, workspace_model::Timestamp) {
    let due = fixture.clock.utc_now() + chrono::Duration::seconds(60);
    let operation = Operation::SetWake {
        target: session(&fixture.caller),
        due_at: due,
        text: "identity wake".to_owned().try_into().unwrap(),
    };
    let result = fixture
        .host
        .request_central(
            &fixture.caller,
            CentralMailRequest {
                request_id: id(request_id),
                operation,
            },
        )
        .await
        .unwrap();
    let Result::Done(Answer::WakeSet { wake_id, .. }) = result else {
        panic!("wake refused: {result:?}")
    };
    (wake_id, due)
}

async fn batch(store: &ObjectStorage, sql: &'static str, args: Vec<central_storage::SqlValue>) {
    let result = store
        .run_batch(StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args },
                expect: RowExpectation::Any,
            }],
            alarm: AlarmUpdate::Unchanged,
        })
        .await
        .unwrap();
    assert!(matches!(result, BatchOutcome::Applied { .. }));
}

async fn assert_untouched(store: &ObjectStorage, wake_id: &workspace_ids::WakeId) {
    let rows = store
        .read(Statement {
            sql: "SELECT state,(SELECT COUNT(*) FROM messages) FROM wakes WHERE wake_id=?",
            args: vec![text(wake_id)],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("active"),
            central_storage::SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
}

#[tokio::test]
async fn unprovisioned_alarm_and_recompute_clear_deadline_without_firing() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let (wake_id, due) = set_wake(&fixture, 1500).await;
    assert_eq!(
        runtime.statement_host().alarm().unwrap().unix_millis(),
        due.timestamp_millis()
    );
    batch(
        runtime.statement_host(),
        "DELETE FROM organization_identity",
        vec![],
    )
    .await;
    fixture.clock.set_utc(due);
    assert_eq!(
        runtime.on_alarm().await.unwrap(),
        collaboration_central_core::AlarmResult::default()
    );
    assert_eq!(runtime.statement_host().alarm(), None);
    assert_untouched(runtime.statement_host(), &wake_id).await;
    let result = runtime
        .statement_host()
        .run_batch(StatementBatch {
            statements: vec![],
            alarm: AlarmUpdate::SetFromQuery(Statement {
                sql: "SELECT ?",
                args: vec![central_storage::SqlValue::Integer(
                    due.timestamp_millis().try_into().unwrap(),
                )],
            }),
        })
        .await
        .unwrap();
    assert!(matches!(result, BatchOutcome::Applied { .. }));
    assert!(runtime.statement_host().alarm().is_some());
    runtime.recompute_alarm().await.unwrap();
    assert_eq!(runtime.statement_host().alarm(), None);
    assert_untouched(runtime.statement_host(), &wake_id).await;
    fixture.host.close().await;
}

#[tokio::test]
async fn matching_stamp_restores_timer_on_startup() {
    let mut fixture = Fixture::new().await;
    let (wake_id, due) = set_wake(&fixture, 1501).await;
    fixture.reopen().await;
    let runtime = fixture
        .host
        .central(&fixture.caller.organization_id)
        .await
        .unwrap();
    let rows = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT id,organization_statement FROM organization_identity",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text(fixture.caller.organization_id.as_str()),
            text("fixture organization statement")
        ]]
    );
    assert_eq!(
        runtime.statement_host().alarm().unwrap().unix_millis(),
        due.timestamp_millis()
    );
    assert_untouched(runtime.statement_host(), &wake_id).await;
    fixture.host.close().await;
}

#[tokio::test]
async fn invalid_stamp_rejects_alarm_and_startup_without_business_effects() {
    for (number, sql, args, expected_column) in [
        (
            1510,
            "UPDATE organization_identity SET id=?",
            vec![text(id::<workspace_model::WorkspaceId>(1514))],
            "organization_identity.id",
        ),
        (
            1511,
            "INSERT INTO organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,'fixture second statement',0,0)",
            vec![text(id::<workspace_model::WorkspaceId>(1515))],
            "organization_identity.id",
        ),
        (
            1512,
            "UPDATE organization_identity SET id='not-an-id'",
            vec![],
            "organization_identity.id",
        ),
        (
            1513,
            "UPDATE organization_identity SET organization_statement=''",
            vec![],
            "organization_identity.organization_statement",
        ),
    ] {
        let fixture = Fixture::new().await;
        let runtime = fixture
            .host
            .central(&fixture.caller.organization_id)
            .await
            .unwrap();
        let (wake_id, due) = set_wake(&fixture, number).await;
        let prior_alarm = runtime.statement_host().alarm();
        batch(runtime.statement_host(), sql, args).await;
        let stamp_before = runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT id,organization_statement FROM organization_identity ORDER BY id",
                args: vec![],
            })
            .await
            .unwrap()
            .rows;
        fixture.clock.set_utc(due);
        let expected = StoreFailure::Corrupt {
            reason: CorruptReason::BadValue {
                column: expected_column.to_owned(),
            },
        };
        assert_eq!(runtime.on_alarm().await.unwrap_err(), expected);
        assert_eq!(runtime.recompute_alarm().await.unwrap_err(), expected);
        assert_eq!(runtime.statement_host().alarm(), prior_alarm);
        assert_untouched(runtime.statement_host(), &wake_id).await;
        fixture.host.close().await;
        let result = TokioWorkspaceHost::open(fixture.root.path(), fixture.clock.clone()).await;
        assert!(matches!(result, Err(ref error) if *error == expected));
        let raw = ObjectStorage::new(
            fixture.root.path().join("central").join(format!(
                "{}.sqlite",
                fixture.caller.organization_id.as_str()
            )),
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        );
        assert_untouched(&raw, &wake_id).await;
        let stamp_after = raw
            .read(Statement {
                sql: "SELECT id,organization_statement FROM organization_identity ORDER BY id",
                args: vec![],
            })
            .await
            .unwrap()
            .rows;
        assert_eq!(stamp_after, stamp_before);
        raw.close().await;
    }
}
