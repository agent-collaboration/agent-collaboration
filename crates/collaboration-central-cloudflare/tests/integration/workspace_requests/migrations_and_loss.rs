use super::*;
use central_storage::{StoreCall, StoreFailure};
#[tokio::test]
async fn business_upgrade_failure_preserves_both_owners_and_every_history_column() {
    let f = Fixture::new().await;
    f.area(880).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let history = || Statement {
        sql: "SELECT version,description,installed_on,success,hex(checksum) AS checksum,execution_time FROM _sqlx_migrations ORDER BY version",
        args: vec![],
    };
    let central = runtime.read(history()).await.unwrap();
    let workspace = runtime
        .inspect_workspace(&f.workspace, history())
        .await
        .unwrap();
    assert_eq!(central.rows.len(), 2);
    assert_eq!(workspace.rows.len(), 1);
    assert_eq!(central.columns.len(), 6);
    assert_eq!(workspace.columns.len(), 6);
    let failed: Result<(), String> = runtime
        .control("central-failing-upgrade", &())
        .await
        .unwrap();
    assert!(failed.is_err());
    assert_eq!(runtime.read(history()).await.unwrap(), central);
    let _: bool = runtime.control("reset-store", &f.workspace).await.unwrap();
    let failed: Result<central_storage::StoreReply, StoreFailure> = runtime
        .control(
            "store-failing-upgrade",
            &serde_json::json!({"workspace":f.workspace,"call":StoreCall::Read(history())}),
        )
        .await
        .unwrap();
    assert_eq!(failed, Err(StoreFailure::Unavailable));
    let _: bool = runtime.control("reset-store", &f.workspace).await.unwrap();
    assert_eq!(
        runtime
            .inspect_workspace(&f.workspace, history())
            .await
            .unwrap(),
        workspace
    );
    for central_store in [true, false] {
        let query = Statement {
            sql: "SELECT name FROM sqlite_master WHERE name IN ('rollback_probe','operations','journal','widgets')",
            args: vec![],
        };
        let rows = if central_store {
            runtime.read(query).await.unwrap()
        } else {
            runtime
                .inspect_workspace(&f.workspace, query)
                .await
                .unwrap()
        };
        assert!(rows.rows.is_empty());
    }
    f.host.close().await;
}
#[tokio::test]
async fn lost_read_before_effect_is_unavailable_and_not_cached() {
    let f = Fixture::new().await;
    f.area(890).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let _: bool = runtime.control("lose-read", &()).await.unwrap();
    let operation = m::WorkspaceOperation::CreateArea {
        name: name("After read loss"),
    };
    assert_eq!(
        f.call(891, operation.clone()).await.result,
        m::WorkspaceResult::CentralUnavailable
    );
    let ledger = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT request_id FROM request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(891))],
            },
        )
        .await
        .unwrap();
    assert!(ledger.rows.is_empty());
    let retry = f.call(891, operation.clone()).await;
    assert!(matches!(
        retry.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    assert_eq!(retry, f.call(891, operation).await);
    f.host.close().await;
}
#[tokio::test]
async fn cold_concurrent_requests_share_the_reopened_org_turn_owner() {
    let mut f = Fixture::new().await;
    let area = f.area(920).await;
    let thread = f.thread(921, area).await;
    let prior = f
        .call(
            922,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "Before".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created {
        position: before, ..
    }) = prior.result
    else {
        panic!("post");
    };
    f.clock.0.fetch_sub(100_000, Ordering::SeqCst);
    f.reopen().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let mut pending = vec![];
    for request in 923..927 {
        let request = m::WorkspaceRequest {
            request_id: Some(id(request)),
            workspace_id: Some(f.workspace.clone()),
            operation: m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "Cold concurrent write".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        };
        pending.push(tokio::spawn({
            let runtime = runtime.clone();
            let caller = f.caller.clone();
            async move { runtime.request(&caller, request).await.unwrap() }
        }));
    }
    let mut positions = std::collections::BTreeSet::new();
    for pending in pending {
        let reply = pending.await.unwrap();
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { position, .. }) = reply.result
        else {
            panic!("cold write did not apply: {reply:?}");
        };
        assert!(position > before);
        positions.insert(position);
    }
    assert_eq!(positions.len(), 4);
    f.host.close().await;
}
