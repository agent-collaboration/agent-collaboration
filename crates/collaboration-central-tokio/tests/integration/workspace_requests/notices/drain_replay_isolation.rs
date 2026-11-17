//! A committed notice write keeps its answer and replay when its drain is corrupt.
use super::*;

#[tokio::test]
async fn committed_write_and_replay_succeed_even_when_its_drain_is_corrupt() {
    let f = Fixture::new().await;
    let area = f.area(8240).await;
    let thread = f.thread(8241, area).await;
    let r = f.host.central(&f.caller.organization_id).await.unwrap();
    let mut held = r
        .pause_points()
        .hold(&f.workspace, PausePoint::AfterStoreApplied);
    let operation = post(&thread, vec![]);
    let request = m::WorkspaceRequest {
        request_id: Some(id(8242)),
        workspace_id: Some(f.workspace.clone()),
        operation,
    };
    let pending = tokio::spawn({
        let r = r.clone();
        let caller = f.caller.clone();
        let request = request.clone();
        async move { r.request(&caller, request).await }
    });
    held.until_held().await;
    f.workspace_batch(
        "UPDATE changes SET change_kind='bad-kind' WHERE change_id=(SELECT change_id FROM changes WHERE thread_id=? AND change_kind='posted' ORDER BY position DESC LIMIT 1)",
        vec![text(&thread)],
    )
    .await;
    held.release();
    let first = pending
        .await
        .unwrap()
        .expect("committed answer survives drain corruption");
    assert!(matches!(
        first.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    let replay = r
        .request(&f.caller, request)
        .await
        .expect("replay returns cached decision");
    assert_eq!(replay, first);
    let ledger = r
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT COUNT(*) FROM request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(8242))],
            },
        )
        .await
        .unwrap();
    assert_eq!(
        ledger.rows,
        vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
    );
    // Re-pointed: the failed drain's retry is the unit's attempt record, due at
    // the next five-minute mark.
    let next_mark = chrono::DateTime::from_timestamp_millis(
        (f.clock.utc_now().timestamp_millis().div_euclid(300_000) + 1) * 300_000,
    )
    .unwrap();
    let record = r
        .statement_host()
        .read(Statement {
            sql: "SELECT state,failure_count,next_try_at FROM central_work_attempts WHERE machine_kind='legacy_workspace_drains' AND subject_key='all'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        record.rows,
        vec![vec![
            text("failing"),
            SqlValue::Integer(1.try_into().unwrap()),
            text(next_mark.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        ]]
    );
    f.host.close().await;
}
