use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
#[tokio::test]
async fn live_admission_refuses_revoked_expired_wrong_credential_and_archived_directory_without_domain_writes()
 {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let operation = m::WorkspaceOperation::CreateArea {
        name: name("Must not be written"),
    };
    let mut wrong = f.caller.clone();
    wrong.credential_hash = "incorrect fixture digest".to_owned();
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
    let edits = [
        (
            "UPDATE machines SET state='revoked' WHERE machine_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::MachineRevoked,
            "UPDATE machines SET state='enrolled' WHERE machine_id=?",
        ),
        (
            "UPDATE machine_credentials SET expires_at='2000-01-01T00:00:00Z' WHERE machine_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::MachineRevoked,
            "UPDATE machine_credentials SET expires_at='2030-01-01T00:00:00Z' WHERE machine_id=?",
        ),
        (
            "UPDATE workspaces SET state='archived' WHERE workspace_id=?",
            text(&f.workspace),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE workspaces SET state='active' WHERE workspace_id=?",
        ),
        (
            "UPDATE machines SET allowed_workspaces='[]' WHERE machine_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
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
