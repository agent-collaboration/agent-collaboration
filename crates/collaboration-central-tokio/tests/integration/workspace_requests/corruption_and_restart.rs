use super::*;
use central_storage::{CorruptReason, RowExpectation, StoreFailure};
#[tokio::test]
async fn role_replacement_rolls_back_after_end_old_and_lost_success_replays_before_authorization() {
    let f = Fixture::new().await;
    let area = f.area(400).await;
    let project = f.project(401, area).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let original = f
        .call(
            0,
            m::WorkspaceOperation::Show {
                item: m::ItemRef::Project(project.clone()),
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Item {
        item: m::ItemView::Project(original),
    }) = original.result
    else {
        panic!("project");
    };
    let mut other = session(&f.caller);
    other.session_id = "other-session".to_owned().try_into().unwrap();
    let operation = m::WorkspaceOperation::GrantProjectRole {
        project_id: project.clone(),
        role: m::ProjectRole::Lead,
        holder: m::ActorRef::Session(other),
    };
    f.workspace_execute("CREATE TRIGGER reject_lead_fixture BEFORE INSERT ON project_roles WHEN NEW.role='lead' BEGIN SELECT RAISE(ABORT,'fixture failure after end old'); END",vec![],RowExpectation::Any).await;
    let failed = f.call(402, operation.clone()).await;
    assert!(
        matches!(
            failed.result,
            m::WorkspaceResult::Conflict | m::WorkspaceResult::CentralUnavailable
        ),
        "{failed:?}"
    );
    let rows=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT holder FROM project_roles WHERE project_id=? AND role='lead' AND ended_at IS NULL",args:vec![text(&project)]}).await.unwrap();
    assert_eq!(rows.rows, vec![vec![text(&original.lead)]]);
    let log = runtime
        .inspect_workspace(
            &f.workspace,
            Statement {
                sql: "SELECT request_id FROM request_log WHERE request_id=?",
                args: vec![text(id::<m::RequestId>(402))],
            },
        )
        .await
        .unwrap();
    assert!(log.rows.is_empty());
    f.workspace_execute(
        "DROP TRIGGER reject_lead_fixture",
        vec![],
        RowExpectation::Any,
    )
    .await;
    runtime.lose_reply_after_apply(id(402));
    assert_eq!(
        f.call(402, operation.clone()).await.result,
        m::WorkspaceResult::OutcomeUnknown
    );
    let applied=runtime.inspect_workspace(&f.workspace,Statement{sql:"SELECT holder FROM project_roles WHERE project_id=? AND role='lead' AND ended_at IS NULL",args:vec![text(&project)]}).await.unwrap();
    assert_eq!(applied.rows.len(), 1);
    assert_ne!(
        applied.rows,
        vec![vec![text(&original.lead)]],
        "replacement committed before its answer went missing"
    );
    let first = f.call(402, operation.clone()).await;
    assert!(matches!(
        first.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    assert_eq!(first, f.call(402, operation.clone()).await);
    assert!(matches!(
        f.call(403, operation).await.result,
        m::WorkspaceResult::Refused(m::RefusalReason::NotLead { .. })
    ));
    f.host.close().await;
}
#[tokio::test]
async fn invalid_role_input_and_stored_role_rows_fail_closed() {
    let f = Fixture::new().await;
    let area = f.area(410).await;
    let project = f.project(411, area).await;
    let deliverable = f.deliverable(412, project.clone()).await;
    for (kind, key, target, role, allowed) in [
        (
            "grantProjectRole",
            "projectId",
            project.to_string(),
            "implementer",
            "lead",
        ),
        (
            "grantDeliverableRole",
            "deliverableId",
            deliverable.to_string(),
            "lead",
            "implementer",
        ),
        (
            "grantProjectRole",
            "projectId",
            project.to_string(),
            "owner",
            "advisor",
        ),
    ] {
        let raw = serde_json::json!({"kind":kind,key:target,"role":role,"holder":{"kind":"me"}});
        let error = serde_json::from_value::<m::WorkspaceOperation>(raw)
            .unwrap_err()
            .to_string();
        assert!(error.contains(allowed), "{error}");
    }
    f.call(
        413,
        m::WorkspaceOperation::GrantDeliverableRole {
            deliverable_id: deliverable.clone(),
            role: m::DeliverableRole::Implementer,
            holder: m::ActorRef::Me,
        },
    )
    .await;
    f.workspace_batch(
        "UPDATE deliverable_roles SET role='owner' WHERE deliverable_id=?",
        vec![text(&deliverable)],
    )
    .await;
    let result = f
        .host
        .request(
            &f.caller,
            m::WorkspaceRequest {
                request_id: Some(id(414)),
                workspace_id: Some(f.workspace.clone()),
                operation: m::WorkspaceOperation::GrantDeliverableRole {
                    deliverable_id: deliverable,
                    role: m::DeliverableRole::Reviewer,
                    holder: m::ActorRef::Me,
                },
            },
        )
        .await;
    assert!(
        matches!(result,Err(StoreFailure::Corrupt{reason:CorruptReason::BadValue{column}}) if column=="deliverable_roles.role")
    );
    f.host.close().await;
}
#[tokio::test]
async fn restart_with_clock_behind_keeps_change_positions_and_current_only_rows() {
    let mut f = Fixture::new().await;
    let area = f.area(420).await;
    let thread = f.thread(421, area).await;
    let original = f
        .call(
            422,
            m::WorkspaceOperation::ThreadPost {
                thread_id: thread.clone(),
                body: "Before".to_owned().try_into().unwrap(),
                addressees: vec![],
            },
        )
        .await;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::Created {
        position: before, ..
    }) = original.result
    else {
        panic!("post");
    };
    f.clock.0.fetch_sub(100_000, Ordering::SeqCst);
    f.reopen().await;
    for request in [423, 424] {
        let later = f
            .call(
                request,
                m::WorkspaceOperation::ThreadPost {
                    thread_id: thread.clone(),
                    body: "After".to_owned().try_into().unwrap(),
                    addressees: vec![],
                },
            )
            .await;
        assert!(
            matches!(later.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Created{position,..}) if position>before)
        );
    }
    let inbox = f
        .call(
            0,
            m::WorkspaceOperation::Inbox {
                after: Some(before),
                limit: 10.try_into().unwrap(),
            },
        )
        .await;
    assert!(
        matches!(inbox.result,m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox{entries,..}) if entries.len()==2 && entries[0].position<entries[1].position)
    );
    let runtime = f.host.central(&f.caller.organization_id).await;
    for table in ["deliverables", "changes"] {
        let schema = runtime
            .inspect_workspace(
                &f.workspace,
                Statement {
                    sql: "SELECT name FROM pragma_table_info(?)",
                    args: vec![text(table)],
                },
            )
            .await
            .unwrap();
        assert!(!schema.rows.iter().any(|row| row == &vec![text("status")]
            || row == &vec![text("body")]
            || row == &vec![text("snapshot")]));
    }
    f.host.close().await;
}
