use super::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, RowExpectation, StatementBatch,
};

#[tokio::test]
async fn non_agent_addressees_are_counted_as_skipped_and_change_remains_in_workspace() {
    let f = Fixture::new().await;
    let area = f.area(7220).await;
    let thread = f.thread(7221, area).await;
    let (observer, agent) = joined_observer(&f, &thread).await;
    let person: m::ActorIdentityId = id(7222);
    let component: m::ActorIdentityId = id(7223);
    let identities = [
        (
            &person,
            "person",
            m::Identity::Person {
                person_id: id(7224),
            },
        ),
        (
            &component,
            "routerComponent",
            m::Identity::RouterComponent {
                machine_id: f.caller.caller.machine_id().clone(),
                component: name("Router"),
            },
        ),
    ];
    let runtime = f.host.central(&f.caller.organization_id).await;
    let statements = identities.into_iter().map(|(actor, kind, identity)| GuardedStatement {
        statement: Statement { sql: "INSERT INTO actor_identities(actor_identity_id,identity_kind,identity,created_at,updated_at) VALUES (?,?,?,?,?)", args: vec![text(actor), text(kind), text(serde_json::to_string(&identity).unwrap()), text("2026-11-13T00:00:00Z"), text("2026-11-13T00:00:00Z")] }, expect: RowExpectation::Exactly(1),
    }).collect();
    assert!(matches!(
        runtime
            .statement_host()
            .run_batch(StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    for (number, actor) in [(7225, &person), (7226, &component)] {
        f.workspace_batch("INSERT INTO thread_members(thread_member_id,thread_id,actor_identity_id,member_role,notifications,left_at,created_at,updated_at) VALUES (?,?,?,'observer',1,NULL,?,?)", vec![text(id::<m::ThreadMemberId>(number)), text(&thread), text(actor), text("2026-11-13T00:00:00Z"), text("2026-11-13T00:00:00Z")]).await;
    }
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        let operation = post(
            &thread,
            [&person, &component, &agent]
                .into_iter()
                .map(|actor| m::Addressee::Actor {
                    actor_identity_id: actor.clone(),
                })
                .collect(),
        );
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(7227)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    let reset: std::result::Result<bool, _> = runtime.control("crash", &()).await;
    assert!(reset.is_err());
    pending.abort();
    let _ = pending.await;
    let result = runtime.drain_workspace(&f.workspace).await.unwrap();
    assert_eq!(result.skipped_recipients, 2);
    assert_eq!(result.notices_queued, 1);
    let rows = runtime.statement_host().read(Statement { sql: "SELECT recipient_actor FROM messages WHERE source_thread_id=? AND message_kind='notice'", args: vec![text(&thread)] }).await.unwrap();
    assert_eq!(rows.rows, vec![vec![text(&agent)]]);
    let workspace_rows = runtime.inspect_workspace(&f.workspace, Statement {
        sql: "SELECT c.position,m.actor_identity_id FROM changes c JOIN thread_members m ON m.thread_id=c.thread_id WHERE c.change_kind='posted' AND c.thread_id=? AND m.actor_identity_id IN (?,?) AND m.left_at IS NULL AND m.notifications=1 ORDER BY m.actor_identity_id",
        args: vec![text(&thread), text(&person), text(&component)],
    }).await.unwrap();
    assert_eq!(workspace_rows.rows.len(), 2);
    assert_eq!(workspace_rows.rows[0][0], workspace_rows.rows[1][0]);
    assert_eq!(workspace_rows.rows[0][1], text(&person));
    assert_eq!(workspace_rows.rows[1][1], text(&component));
    let cursor = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
            args: vec![text(&f.workspace)],
        })
        .await
        .unwrap();
    assert_eq!(cursor.rows[0][0], workspace_rows.rows[0][0]);
    let SqlValue::Integer(position) = &workspace_rows.rows[0][0] else {
        panic!("posted position")
    };
    let posted_position: m::ChangePosition =
        u64::try_from(position.get()).unwrap().try_into().unwrap();
    let mut person_caller = f.caller.clone();
    person_caller.caller = m::Caller::Person {
        machine_id: f.caller.caller.machine_id().clone(),
        person_id: id(7224),
    };
    for caller in [&person_caller, &observer] {
        let reply = call(
            &f.host,
            caller,
            Some(f.workspace.clone()),
            0,
            m::WorkspaceOperation::Inbox {
                after: None,
                limit: 100.try_into().unwrap(),
            },
        )
        .await;
        let m::WorkspaceResult::Done(m::WorkspaceAnswer::Inbox { entries, .. }) = reply.result
        else {
            panic!("inbox refused: {reply:?}")
        };
        assert!(
            entries
                .iter()
                .any(|entry| entry.position == posted_position)
        );
    }
    f.host.close().await;
}
