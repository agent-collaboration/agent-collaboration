use super::fixture::{ConnectedLink, Fixture, receive, send, session_caller};
use central_link_protocol::{DataFrame, RequestScope, RequestStatusAnswer, RequestStatusQuery};
use collaboration_central_conformance::ConformanceResult as Result;
use workspace_model::{WorkspaceOperation, WorkspaceRequest};

#[tokio::test]
async fn worker_machine_link_reads_the_retained_decision_from_the_selected_store() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = query_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn query_journey(fixture: &Fixture) -> Result<()> {
    let mut link = ConnectedLink::open(fixture).await?;
    let request_id: workspace_model::RequestId = "019f0000-0000-7000-8000-00000000cf11".parse()?;
    let exchange = link
        .call(WorkspaceRequest {
            request_id: Some(request_id.clone()),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: "Status workspace".to_owned().try_into()?,
            },
        })
        .await?;
    let DataFrame::Reply {
        result: original, ..
    } = exchange.reply
    else {
        return Err("create did not return a Reply".into());
    };
    let central_link_protocol::ReplyPayload::Workspace(workspace_model::WorkspaceReply {
        result:
            workspace_model::WorkspaceResult::Done(workspace_model::WorkspaceAnswer::WorkspaceCreated {
                workspace,
            }),
        ..
    }) = original.as_ref()
    else {
        return Err("create did not retain a workspace".into());
    };
    fixture.allow_workspace(&workspace.workspace_id).await?;
    let cases = [
        (
            RequestScope::Organization,
            RequestStatusAnswer::Answered(*original.clone()),
        ),
        (
            RequestScope::Workspace(workspace.workspace_id.clone()),
            RequestStatusAnswer::NotFound,
        ),
    ];
    for (index, (scope, expected)) in cases.into_iter().enumerate() {
        let correlation_id: central_link_protocol::CorrelationId =
            format!("019f0000-0000-7000-8000-00000000cf{:02x}", 0x12 + index).parse()?;
        let frame = DataFrame::RequestStatus {
            correlation_id: correlation_id.clone(),
            caller: session_caller()?,
            query: RequestStatusQuery {
                request_id: request_id.clone(),
                scope,
            },
        };
        send(&mut link.socket, &frame).await?;
        assert_eq!(
            receive(&mut link.socket).await?,
            DataFrame::RequestStatusReply {
                correlation_id,
                result: Box::new(expected),
            }
        );
    }
    link.close().await
}
