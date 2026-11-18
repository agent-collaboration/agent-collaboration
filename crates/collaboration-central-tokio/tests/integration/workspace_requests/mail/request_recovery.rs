//! Warm request completions repair physical alarms without replacing decisions.
use super::super::{Statement, support::*};
use central_storage::{ReadOutcome, StoreFailure};
use collaboration_central_conformance::ConformanceResult as TestResult;
use router_clock::RuntimeClock;
use std::sync::Arc;
use workspace_model as m;
use workspace_model::central_mail::{
    CentralMailAnswer, CentralMailOperation, CentralMailRequest, CentralMailResult,
};

async fn future_wake(f: &Fixture) -> TestResult<m::Timestamp> {
    let due = f.clock.utc_now() + chrono::Duration::seconds(600);
    let result = f
        .host
        .request_central(
            &f.caller,
            CentralMailRequest {
                request_id: id(8600),
                operation: CentralMailOperation::SetWake {
                    target: session(&f.caller),
                    due_at: due,
                    text: "request recovery".to_owned().try_into()?,
                },
            },
        )
        .await?;
    match result {
        CentralMailResult::Done(CentralMailAnswer::WakeSet { due_at, .. }) if due_at == due => {
            Ok(due)
        }
        _ => Err("fixture future wake was not admitted".into()),
    }
}
async fn drop_alarm(f: &Fixture) -> TestResult<()> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
    let applied = runtime
        .statement_host()
        .run_batch(central_storage::StatementBatch {
            statements: vec![],
            alarm: central_storage::AlarmUpdate::SetFromQuery(Statement {
                sql: "SELECT NULL AS earliest_due",
                args: vec![],
            }),
        })
        .await?;
    if !matches!(applied, central_storage::BatchOutcome::Applied { .. }) {
        return Err("physical alarm clear did not apply".into());
    }
    Ok(())
}
async fn alarm(f: &Fixture) -> TestResult<Option<i64>> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
    Ok(runtime.statement_host().alarm().map(|at| at.unix_millis()))
}
async fn fail_rearm(f: &Fixture) -> TestResult<()> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
    runtime
        .statement_host()
        .fail_next_alarm_batch(StoreFailure::Unavailable);
    Ok(())
}
async fn rows(
    f: &Fixture,
    sql: &'static str,
    args: Vec<central_storage::SqlValue>,
) -> TestResult<ReadOutcome> {
    let runtime = f.host.central(f.caller.organization_id()).await?;
    Ok(runtime
        .statement_host()
        .read(Statement { sql, args })
        .await?)
}
fn create(serial: u32) -> m::WorkspaceRequest {
    m::WorkspaceRequest {
        request_id: Some(id(serial)),
        workspace_id: None,
        operation: m::WorkspaceOperation::CreateWorkspace {
            name: name("Recovery created workspace"),
            description: None,
        },
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real Show on the warm runtime must restore the physically absent future alarm."
)]
#[tokio::test]
async fn warm_show_read_rearms_a_physically_missing_alarm() -> TestResult<()> {
    let f = Fixture::new().await;
    let area = f.area(8601).await;
    let due = future_wake(&f).await?;
    let runtime = f.host.central(f.caller.organization_id()).await?;
    let operation = m::WorkspaceOperation::Show {
        item: m::ItemRef::Area(area),
    };
    let expected = f.call(8602, operation.clone()).await;
    assert!(matches!(
        expected.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })
    ));
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    drop_alarm(&f).await?;
    assert_eq!(
        alarm(&f).await?,
        None,
        "physical alarm must really be absent before the read"
    );
    let actual = f.call(8603, operation).await;
    assert_eq!(actual, expected);
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    let after = f.host.central(f.caller.organization_id()).await?;
    assert!(
        Arc::ptr_eq(&runtime, &after),
        "recovery must use the same cached native runtime"
    );
    f.host.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Generic CreateWorkspace and exact replay must both repair the alarm without cold initialization."
)]
#[tokio::test]
async fn warm_generic_create_and_replay_each_rearm_a_physically_missing_alarm() -> TestResult<()> {
    let f = Fixture::new().await;
    let due = future_wake(&f).await?;
    let runtime = f.host.central(f.caller.organization_id()).await?;
    let request = create(8610);
    drop_alarm(&f).await?;
    assert_eq!(alarm(&f).await?, None);
    let first = f.host.request(&f.caller, request.clone()).await?;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        &first.result
    else {
        return Err("generic CreateWorkspace did not commit".into());
    };
    let created_id = workspace.workspace_id.clone();
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    let before=rows(&f,"SELECT (SELECT COUNT(*) FROM workspaces WHERE workspace_id=?),(SELECT COUNT(*) FROM request_log WHERE request_id=?)",vec![text(&created_id),text(id::<m::RequestId>(8610))]).await?.rows;
    assert_eq!(
        before,
        vec![vec![
            central_storage::SqlValue::Integer(1_i64.try_into()?),
            central_storage::SqlValue::Integer(1_i64.try_into()?)
        ]]
    );
    drop_alarm(&f).await?;
    assert_eq!(
        alarm(&f).await?,
        None,
        "replay must start with a newly dropped physical alarm"
    );
    assert_eq!(f.host.request(&f.caller, request).await?, first);
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    assert_eq!(rows(&f,"SELECT (SELECT COUNT(*) FROM workspaces WHERE workspace_id=?),(SELECT COUNT(*) FROM request_log WHERE request_id=?)",vec![text(&created_id),text(id::<m::RequestId>(8610))]).await?.rows,before);
    let after = f.host.central(f.caller.organization_id()).await?;
    assert!(
        Arc::ptr_eq(&runtime, &after),
        "recovery must use the same cached native runtime"
    );
    f.host.close().await;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A completion-only rearm failure must not replace a committed Create decision or its retained replay."
)]
#[tokio::test]
async fn completion_rearm_failure_preserves_committed_create_and_exact_replay() -> TestResult<()> {
    let f = Fixture::new().await;
    let area = f.area(8620).await;
    let due = future_wake(&f).await?;
    let runtime = f.host.central(f.caller.organization_id()).await?;
    let request = create(8621);
    drop_alarm(&f).await?;
    assert_eq!(alarm(&f).await?, None);
    fail_rearm(&f).await?;
    let first = f.host.request(&f.caller, request.clone()).await?;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        &first.result
    else {
        return Err("rearm failure replaced the committed Create answer".into());
    };
    let created_id = workspace.workspace_id.clone();
    assert_eq!(
        alarm(&f).await?,
        None,
        "the injected completion batch must actually fail to rearm"
    );
    let before = rows(
        &f,
        "SELECT workspace_id,name,state,created_at,updated_at FROM workspaces WHERE workspace_id=?",
        vec![text(&created_id)],
    )
    .await?
    .rows;
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].get(1), Some(&text("Recovery created workspace")));
    drop_alarm(&f).await?;
    assert_eq!(alarm(&f).await?, None);
    fail_rearm(&f).await?;
    assert_eq!(
        f.host.request(&f.caller, request).await?,
        first,
        "cached success must survive another completion-only rearm failure"
    );
    assert_eq!(alarm(&f).await?, None);
    assert_eq!(rows(&f,"SELECT workspace_id,name,state,created_at,updated_at FROM workspaces WHERE workspace_id=?",vec![text(&created_id)]).await?.rows,before);
    let ledger = rows(
        &f,
        "SELECT COUNT(*) FROM request_log WHERE request_id=?",
        vec![text(id::<m::RequestId>(8621))],
    )
    .await?
    .rows;
    assert_eq!(
        ledger,
        vec![vec![central_storage::SqlValue::Integer(1_i64.try_into()?)]]
    );
    // A later ordinary read repairs the same warm runtime after the one-shot fault.
    let shown = f
        .call(
            8622,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Area(area),
            },
        )
        .await;
    assert!(matches!(
        shown.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Item { .. })
    ));
    assert_eq!(alarm(&f).await?, Some(due.timestamp_millis()));
    let after = f.host.central(f.caller.organization_id()).await?;
    assert!(
        Arc::ptr_eq(&runtime, &after),
        "recovery must use the same cached native runtime"
    );
    f.host.close().await;
    Ok(())
}
