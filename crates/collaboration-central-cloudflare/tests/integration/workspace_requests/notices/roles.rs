use super::*;

#[tokio::test]
async fn role_grant_notifies_subject_and_role_address_uses_holder_at_drain() {
    let f = Fixture::new().await;
    let area = f.area(7200).await;
    let project = f.project(7201, area.clone()).await;
    let thread = f.thread(7202, area).await;
    let (_, old_holder) = joined_observer(&f, &thread).await;
    let runtime = f.host.central(&f.caller.organization_id).await;
    let actors = runtime
        .statement_host()
        .read(Statement {
            sql: "SELECT actor_identity_id,identity FROM actor_identities",
            args: vec![],
        })
        .await
        .unwrap();
    let current_holder: m::ActorIdentityId = actors
        .rows
        .iter()
        .find_map(|row| match (&row[0], &row[1]) {
            (SqlValue::Text(id), SqlValue::Text(identity)) if identity.contains("test-session") => {
                Some(id.parse().unwrap())
            }
            _ => None,
        })
        .unwrap();
    let granted = f
        .call(
            7203,
            m::WorkspaceOperation::GrantProjectRole {
                project_id: project.clone(),
                role: m::ProjectRole::Advisor,
                holder: m::ActorRef::Actor(old_holder.clone()),
            },
        )
        .await;
    assert!(matches!(
        granted.result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Updated { .. })
    ));
    let grant = runtime.statement_host().read(Statement {
        sql: "SELECT COUNT(*) FROM messages WHERE recipient_actor=? AND source_workspace_id=? AND source_thread_id IS NULL AND message_kind='notice' AND state='queued'",
        args: vec![text(&old_holder), text(&f.workspace)],
    }).await.unwrap();
    assert_eq!(
        grant.rows,
        vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
    );
    let pause = serde_json::json!({"workspace":f.workspace,"point":"afterStoreApplied"});
    let _: bool = runtime.control("hold", &pause).await.unwrap();
    let pending = tokio::spawn({
        let runtime = runtime.clone();
        let caller = f.caller.clone();
        let workspace = f.workspace.clone();
        let operation = post(
            &thread,
            vec![m::Addressee::ProjectRole {
                project_id: project.clone(),
                role: m::ProjectRole::Advisor,
            }],
        );
        async move {
            runtime
                .request(
                    &caller,
                    m::WorkspaceRequest {
                        request_id: Some(id(7204)),
                        workspace_id: Some(workspace),
                        operation,
                    },
                )
                .await
                .unwrap()
        }
    });
    let _: bool = runtime.control("until-held", &pause).await.unwrap();
    // Fixture-stamp the current assignment between workspace apply and its drain snapshot.
    f.workspace_batch("UPDATE project_roles SET holder=? WHERE project_id=? AND role='advisor' AND ended_at IS NULL", vec![text(&current_holder), text(&project)]).await;
    let _: bool = runtime.control("release", &pause).await.unwrap();
    assert!(matches!(
        pending.await.unwrap().result,
        m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
    ));
    let rows = runtime.statement_host().read(Statement {
        sql: "SELECT recipient_actor FROM messages WHERE source_workspace_id=? AND source_thread_id=? AND message_kind='notice'",
        args: vec![text(&f.workspace), text(&thread)],
    }).await.unwrap();
    assert_eq!(rows.rows, vec![vec![text(&current_holder)]]);
    f.host.close().await;
}
