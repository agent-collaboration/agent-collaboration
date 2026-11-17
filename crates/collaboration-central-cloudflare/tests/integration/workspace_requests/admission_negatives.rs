use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_central_conformance::signin_fixtures as signed;
#[tokio::test]
async fn live_admission_refuses_revoked_expired_wrong_token_and_archived_directory_without_domain_writes()
 {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let operation = m::WorkspaceOperation::CreateArea {
        name: name("Must not be written"),
    };
    let mut wrong = f.caller.clone();
    wrong.token = signed::tamper_signature(&wrong.token).unwrap();
    assert_eq!(
        call(
            &f.host,
            &wrong,
            Some(f.workspace.clone()),
            960,
            operation.clone()
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::MachineRevoked)
    );
    let mut expired = f.caller.clone();
    expired.token = signed::machine_token(
        expired.organization_id.as_str(),
        &expired.caller.machine_id().to_string(),
        &expired.enrollment_id,
        FIXTURE_NOW - 600,
        FIXTURE_NOW,
    )
    .unwrap();
    assert_eq!(
        call(
            &f.host,
            &expired,
            Some(f.workspace.clone()),
            959,
            operation.clone()
        )
        .await
        .result,
        m::WorkspaceResult::Refused(m::RefusalReason::MachineRevoked)
    );
    let edits = [
        (
            "UPDATE machines SET state='revoked' WHERE installation_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::MachineRevoked,
            "UPDATE machines SET state='enrolled' WHERE installation_id=?",
        ),
        (
            "UPDATE workspaces SET state='archived' WHERE workspace_id=?",
            text(&f.workspace),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE workspaces SET state='active' WHERE workspace_id=?",
        ),
        (
            "UPDATE machines SET allowed_workspaces='[]' WHERE installation_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE machines SET allowed_workspaces=? WHERE installation_id=?",
        ),
    ];
    for (index, (sql, key, refusal, restore)) in edits.into_iter().enumerate() {
        let run = |sql, args| StatementBatch {
            statements: vec![GuardedStatement {
                statement: Statement { sql, args },
                expect: RowExpectation::Exactly(1),
            }],
            alarm: AlarmUpdate::Unchanged,
        };
        assert!(matches!(
            runtime
                .statement_host()
                .run_batch(run(sql, vec![key.clone()]))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
        assert_eq!(
            f.call(961 + u32::try_from(index).unwrap(), operation.clone())
                .await
                .result,
            m::WorkspaceResult::Refused(refusal)
        );
        let args = if restore.contains("allowed_workspaces") {
            vec![text(serde_json::to_string(&[&f.workspace]).unwrap()), key]
        } else {
            vec![key]
        };
        assert!(matches!(
            runtime
                .statement_host()
                .run_batch(run(restore, args))
                .await
                .unwrap(),
            BatchOutcome::Applied { .. }
        ));
    }
    let counts=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT (SELECT COUNT(*) FROM areas) AS areas,(SELECT COUNT(*) FROM changes) AS changes,(SELECT COUNT(*) FROM request_log) AS log",args:vec![]}).await.unwrap();
    for cell in &counts.rows[0] {
        assert_eq!(
            *cell,
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(0).unwrap())
        );
    }
    assert!(matches!(
        f.call(970, operation).await.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    f.host.close().await;
}
