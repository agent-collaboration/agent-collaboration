use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};
use collaboration_central_conformance::signin_fixtures as signed;
#[tokio::test]
async fn live_admission_refuses_revoked_expired_wrong_credential_and_archived_directory_without_domain_writes()
 {
    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let schema = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT name, pk FROM pragma_table_info('login_machines') WHERE name IN ('machine_id', 'installation_id') ORDER BY cid",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        schema.rows,
        vec![vec![
            text("machine_id"),
            central_storage::SqlValue::Integer(central_storage::SafeInteger::new(1).unwrap())
        ]]
    );
    let link = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT \"table\", \"to\" FROM pragma_foreign_key_list('mail_messages') WHERE \"from\"='target_machine_id'",
            args: vec![],
        })
        .await
        .unwrap();
    assert_eq!(
        link.rows,
        vec![vec![text("login_machines"), text("machine_id")]]
    );
    let legacy = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM sqlite_schema WHERE type='table' AND name='machine_credentials'",
            args: vec![],
        })
        .await
        .unwrap();
    assert!(legacy.rows.is_empty());
    let operation = m::WorkspaceOperation::CreateArea {
        name: name("Must not be written"),
        description: "Test description".to_owned().try_into().unwrap(),
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
            "UPDATE login_machines SET state='revoked' WHERE machine_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::MachineRevoked,
            "UPDATE login_machines SET state='enrolled' WHERE machine_id=?",
        ),
        (
            "UPDATE directory_workspaces SET state='archived' WHERE workspace_id=?",
            text(&f.workspace),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE directory_workspaces SET state='active' WHERE workspace_id=?",
        ),
        (
            "UPDATE login_machines SET allowed_workspaces='[]' WHERE machine_id=?",
            text(f.caller.caller.machine_id()),
            m::RefusalReason::WorkspaceNotAllowed,
            "UPDATE login_machines SET allowed_workspaces=? WHERE machine_id=?",
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
    let counts=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT (SELECT COUNT(*) FROM workspace_areas) AS areas,(SELECT COUNT(*) FROM workspace_changes) AS changes,(SELECT COUNT(*) FROM workspace_request_log) AS log",args:vec![]}).await.unwrap();
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

#[tokio::test]
async fn fresh_organization_and_workspace_store_use_the_approved_table_names() {
    use collaboration_central_conformance::workspace_schema_fixture as schema;

    let f = Fixture::new().await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let names = || Statement {
        sql: schema::APPLICATION_TABLE_NAMES,
        args: vec![],
    };
    let organization = runtime.statement_host().read(names()).await.unwrap();
    let workspace = runtime
        .inspect_workspace(&f.workspace, names())
        .await
        .unwrap();
    let expected_organization = schema::organization_test_tables();
    for (observed, expected) in [
        (organization, expected_organization.as_slice()),
        (workspace, schema::WORKSPACE_TABLES),
    ] {
        let expected: Vec<_> = expected.iter().map(|name| vec![text(name)]).collect();
        assert_eq!(observed.rows, expected);
    }
    f.host.close().await;
}
