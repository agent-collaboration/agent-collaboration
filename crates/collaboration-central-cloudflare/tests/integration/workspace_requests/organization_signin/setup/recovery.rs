use super::{owned_rows, support::*};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, SqlValue, Statement,
    StatementBatch,
};

#[tokio::test]
async fn lost_create_answer_recovers_identical_result_after_reopen() {
    let f = ready().await;
    let owner = f.owner();
    let operation = operation(0x741);
    let (body, intent) = create(&operation, "Lost answer", "Retained description");
    let operation_id: workspace_model::RequestId = operation.clone().try_into().unwrap();
    assert!(
        f.runtime()
            .await
            .control::<bool>("lose-reply", &operation_id)
            .await
            .unwrap()
    );
    let lost = f
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(lost.0, 503);
    assert!(lost.1.is_empty());
    let before = stable_effects(&f).await;
    assert_eq!(f.read("SELECT (SELECT COUNT(*) FROM directory_workspaces),(SELECT COUNT(*) FROM runtime_request_log),(SELECT COUNT(*) FROM signin_change_records),(SELECT COUNT(*) FROM admin_used_requests)").await.rows,vec![vec![number(1),number(1),number(2),number(1)]]);
    let retained = f
        .read("SELECT answer FROM runtime_request_log ORDER BY request_id")
        .await
        .rows;
    let Some(SqlValue::Text(retained)) = retained.first().and_then(|row| row.first()) else {
        panic!("retained answer missing")
    };
    let expected: Value = serde_json::from_str(retained).unwrap();
    assert_eq!(
        expected,
        json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Lost answer","description":"Retained description","state":"active"}})
    );
    crash(&f).await;
    let replay = f
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(replay.0, 200);
    assert!(
        replay.1 == retained.as_bytes(),
        "reopen recovery must return the exact first retained answer bytes"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.1).unwrap(),
        expected
    );
    let again = f
        .post_bytes(
            signed::CREATE,
            &body,
            &headers(&owner, &intent, NOW_SECONDS),
        )
        .await;
    assert_eq!(again.0, 200);
    assert!(
        again.1 == replay.1,
        "fresh-proof replays after owning reopen return byte-identical first answer"
    );
    assert!(
        stable_effects(&f).await == before,
        "replay changes no directory, ledger, actor or record"
    );
    assert_eq!(
        f.read("SELECT COUNT(*) FROM admin_used_requests")
            .await
            .rows,
        vec![vec![number(3)]]
    );
    f.close().await;
}

#[tokio::test]
async fn person_create_records_checked_origin_without_machine_rights() {
    let f = ready().await;
    let caller = super::super::super::support::machine_caller(
        f.org.clone(),
        workspace_model::Caller::Session(workspace_ids::SessionRef {
            machine_id: "019f0000-0000-7000-8000-00000000d752".parse().unwrap(),
            session_id: "setup-origin-machine".to_owned().try_into().unwrap(),
        }),
        "019f0000-0000-7000-8000-00000000d753".to_owned(),
        NOW_SECONDS,
    );
    super::super::super::support::seed_enrollment(&f.host, &caller, "[]", NOW_SECONDS).await;
    let before = f
        .read("SELECT * FROM login_machines ORDER BY machine_id")
        .await;
    let approvals = f
        .read("SELECT * FROM login_machine_approvals ORDER BY approval_id")
        .await;
    let operation = operation(0x742);
    assert_eq!(
        submit(&f, &f.owner(), &operation, "Person origin").await.1["result"],
        "applied"
    );
    assert_eq!(f.read("SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM signin_change_records WHERE kind='workspaceCreated'").await.rows,vec![vec![SqlValue::Text(operation.clone()),SqlValue::Text("workspaceCreated".into()),SqlValue::Text("person".into()),SqlValue::Text(signed::OWNER.into()),SqlValue::Text("workspace:create".into()),SqlValue::Text(operation.clone())]]);
    assert_eq!(
        f.read("SELECT * FROM login_machines ORDER BY machine_id")
            .await,
        before
    );
    assert_eq!(
        f.read("SELECT * FROM login_machine_approvals ORDER BY approval_id")
            .await,
        approvals
    );
    let request = workspace_model::WorkspaceRequest {
        request_id: Some(super::support::operation(0x754).try_into().unwrap()),
        workspace_id: Some(operation.clone().try_into().unwrap()),
        operation: workspace_model::WorkspaceOperation::CreateArea {
            name: "Allowed control".to_owned().try_into().unwrap(),
            description: "Test description".to_owned().try_into().unwrap(),
        },
    };
    let denied = f.host.request(&caller, request.clone()).await.unwrap();
    assert!(
        matches!(
            denied.result,
            workspace_model::WorkspaceResult::Refused(
                workspace_model::RefusalReason::WorkspaceNotAllowed
            )
        ),
        "person create grants no ordinary machine allowance"
    );
    f.execute(
        "UPDATE login_machines SET allowed_workspaces=? WHERE machine_id=?",
        vec![
            SqlValue::Text(serde_json::to_string(&vec![&operation]).unwrap()),
            SqlValue::Text(caller.caller.machine_id().to_string()),
        ],
    )
    .await;
    let allowed = f.host.request(&caller, request).await.unwrap();
    assert!(
        matches!(allowed.result, workspace_model::WorkspaceResult::Done(_)),
        "explicit machine allowance still enables its normal request"
    );
    f.close().await;
}

#[tokio::test]
async fn generic_create_trims_names_defaults_description_and_shares_name_uniqueness() {
    let f = ready().await;
    let caller = super::super::super::support::machine_caller(
        f.org.clone(),
        workspace_model::Caller::Session(workspace_ids::SessionRef {
            machine_id: "019f0000-0000-7000-8000-00000000d762".parse().unwrap(),
            session_id: "generic-setup".to_owned().try_into().unwrap(),
        }),
        "019f0000-0000-7000-8000-00000000d763".to_owned(),
        NOW_SECONDS,
    );
    super::super::super::support::seed_enrollment(&f.host, &caller, "[]", NOW_SECONDS).await;
    let request = |serial, name: &str| workspace_model::WorkspaceRequest {
        request_id: Some(operation(serial).try_into().unwrap()),
        workspace_id: None,
        operation: workspace_model::WorkspaceOperation::CreateWorkspace {
            name: name.to_owned().try_into().unwrap(),
        },
    };
    let created = f
        .host
        .request(&caller, request(0x764, "  Shared name  "))
        .await
        .unwrap();
    assert!(matches!(
        created.result,
        workspace_model::WorkspaceResult::Done(
            workspace_model::WorkspaceAnswer::WorkspaceCreated { .. }
        )
    ));
    assert_eq!(
        f.read("SELECT name,description FROM directory_workspaces")
            .await
            .rows,
        vec![vec![
            SqlValue::Text("Shared name".into()),
            SqlValue::Text("".into())
        ]]
    );
    let duplicate = f
        .host
        .request(&caller, request(0x765, "shared NAME"))
        .await
        .unwrap();
    assert!(matches!(
        duplicate.result,
        workspace_model::WorkspaceResult::Refused(
            workspace_model::RefusalReason::WorkspaceNameInUse { .. }
        )
    ));
    assert_eq!(
        submit(&f, &f.owner(), &operation(0x766), "SHARED NAME").await,
        (200, json!({"result":"refused","reason":"nameInUse"}))
    );
    assert_eq!(
        f.read("SELECT COUNT(*) FROM directory_workspaces")
            .await
            .rows,
        vec![vec![number(1)]]
    );
    f.close().await;
}

#[tokio::test]
async fn last_statement_failure_rolls_back_directory_actor_ledger_proof_and_change() {
    let f = ready().await;
    member_floor(&f).await;
    let earlier_operation = operation(0x744);
    assert_eq!(
        submit(&f, &f.owner(), &earlier_operation, "Earlier real create")
            .await
            .1["result"],
        "applied"
    );
    f.clock
        .advance(std::time::Duration::from_secs(86_401))
        .unwrap();
    f.runtime().await;
    let now = NOW_SECONDS + 86_401;
    let mut person = signed::Person::member(ORG, now).unwrap();
    person.permissions = vec!["workspace:create".into()];
    let runtime = f.runtime().await;
    // request_log::retain ends with this DELETE. A real older create makes
    // its expired row eligible, so the final cleanup statement must visit it.
    let outcome=runtime.run_batch(StatementBatch{statements:vec![GuardedStatement{statement:Statement{sql:"CREATE TRIGGER fail_setup_ledger BEFORE DELETE ON runtime_request_log BEGIN SELECT RAISE(ABORT,'fixture last setup statement failure'); END",args:vec![]},expect:RowExpectation::Any}],alarm:AlarmUpdate::Unchanged}).await.unwrap();
    assert!(matches!(outcome, BatchOutcome::Applied { .. }));
    let before = owned_rows(&f).await;
    let operation = operation(0x743);
    let (body, intent) = create(&operation, "Atomic", "atomic description");
    let proof = headers(&person, &intent, now);
    let failed = f.post_bytes(signed::CREATE, &body, &proof).await;
    assert_eq!(failed.0, 503);
    assert!(failed.1.is_empty());
    assert!(
        owned_rows(&f).await == before,
        "final ledger DELETE failure must roll back all six owning tables, including the new person's actor"
    );
    assert!(matches!(
        runtime
            .run_batch(StatementBatch {
                statements: vec![GuardedStatement {
                    statement: Statement {
                        sql: "DROP TRIGGER fail_setup_ledger",
                        args: vec![]
                    },
                    expect: RowExpectation::Any
                }],
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    let healthy = f.post_bytes(signed::CREATE, &body, &proof).await;
    assert_eq!(healthy.0, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&healthy.1).unwrap()["result"],
        "applied",
        "the exact same proof remains fresh after full rollback"
    );
    assert_eq!(f.read("SELECT (SELECT COUNT(*) FROM directory_workspaces),(SELECT COUNT(*) FROM signin_change_records),(SELECT COUNT(*) FROM admin_used_requests),(SELECT COUNT(*) FROM runtime_request_log),(SELECT COUNT(*) FROM access_actor_identities)").await.rows,vec![vec![number(2),number(3),number(2),number(1),number(2)]]);
    let replay = f
        .post_bytes(signed::CREATE, &body, &headers(&person, &intent, now))
        .await;
    assert_eq!(replay.0, 200);
    assert!(
        replay.1 == healthy.1,
        "fresh-proof retry retains the one successfully applied outcome"
    );
    f.close().await;
}
