use super::*;
use central_link_protocol::{
    ReplyPayload, RequestRefusal, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};

#[tokio::test]
async fn request_status_reads_only_its_scope_and_only_for_its_original_caller() {
    let fixture = Fixture::new().await;
    let runtime = fixture
        .host
        .central(fixture.caller.organization_id())
        .await
        .unwrap();
    let caller = fixture.fresh_caller(&fixture.caller).await;
    let submitted = fixture
        .call(
            801,
            m::WorkspaceOperation::CreateArea {
                name: name("Retained area"),
                description: "Create once".to_owned().try_into().unwrap(),
            },
        )
        .await;
    let _ = created(submitted.clone());
    let status_query = RequestStatusQuery {
        request_id: id(801),
        scope: RequestScope::Workspace(fixture.workspace.clone()),
    };
    assert_eq!(
        runtime
            .request_status(&caller, &status_query)
            .await
            .unwrap(),
        RequestStatusAnswer::Answered(ReplyPayload::Workspace(submitted))
    );
    let wrong_store = RequestStatusQuery {
        scope: RequestScope::Organization,
        ..status_query.clone()
    };
    assert_eq!(
        runtime.request_status(&caller, &wrong_store).await.unwrap(),
        RequestStatusAnswer::NotFound
    );

    // Give the second session a real actor on the same machine and access to the same workspace.
    let other_caller = fleet_contracts::WorkspaceCaller::new(
        caller.connection().clone(),
        m::Caller::Session(workspace_ids::SessionRef {
            machine_id: caller.caller().machine_id().clone(),
            session_id: "another-session".to_owned().try_into().unwrap(),
        }),
    )
    .unwrap();
    let refusal = call(
        &fixture.host,
        &other_caller,
        None,
        802,
        m::WorkspaceOperation::CreateWorkspace {
            name: name("Workspace"),
        },
    )
    .await;
    let m::WorkspaceResult::Refused(expected) = refusal.result else {
        panic!("expected name refusal")
    };
    assert_eq!(
        runtime
            .request_status(
                &other_caller,
                &RequestStatusQuery {
                    request_id: id(802),
                    scope: RequestScope::Organization
                }
            )
            .await
            .unwrap(),
        RequestStatusAnswer::Refused(RequestRefusal::Workspace(expected))
    );
    assert_eq!(
        runtime
            .request_status(&other_caller, &status_query)
            .await
            .unwrap(),
        RequestStatusAnswer::NotFound
    );
    fixture.host.close().await;
}
