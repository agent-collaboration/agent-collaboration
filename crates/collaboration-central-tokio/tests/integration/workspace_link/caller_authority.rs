//! Only a session bound to the signed machine may act on a public link.
use super::fixture::{Fixture, INSTALLATION, receive, send};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, LinkRequest, LinkRequestBatch, ReplyPayload,
    RequestPayload, RequestRefusal, RequestScope, RequestStatusAnswer, RequestStatusQuery,
};
use central_storage::Statement;
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use workspace_ids::SessionRef;
use workspace_model::{
    Caller, RefusalReason, WorkspaceOperation, WorkspaceReply, WorkspaceRequest, WorkspaceResult,
};

#[tokio::test]
async fn public_link_refuses_unverified_person_and_foreign_session_callers() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = rejected_callers(&fixture).await;
    fixture.close().await?;
    result
}

async fn rejected_callers(fixture: &Fixture) -> Result<()> {
    let mut connection = fixture.connect().await?;
    if !matches!(
        receive(&mut connection).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ) {
        return Err("machine did not receive Welcome".into());
    }
    let socket = &mut connection;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let store = runtime.runtime().statement_host();
    let forged_person = Caller::Person {
        machine_id: INSTALLATION.parse()?,
        person_id: signed::OWNER.parse()?,
    };
    let foreign_session = Caller::Session(SessionRef {
        machine_id: "019f0000-0000-7000-8000-00000000e051".parse()?,
        session_id: "019f0000-0000-7000-8000-00000000e052"
            .to_owned()
            .try_into()?,
    });
    let counts = || Statement {
        sql: "SELECT (SELECT count(*) FROM directory_workspaces) AS workspaces, (SELECT count(*) FROM runtime_request_log) AS requests",
        args: vec![],
    };
    let before = store.read(counts()).await?;
    let mut batch = Vec::new();
    for (index, (caller, refusal)) in [
        (
            forged_person,
            RefusalReason::InvalidInput {
                field: "caller".to_owned(),
            },
        ),
        (foreign_session, RefusalReason::MachineRevoked),
    ]
    .into_iter()
    .enumerate()
    {
        let request_id = String::from(test_id(index * 10 + 1)?).parse()?;
        let operation = WorkspaceOperation::CreateWorkspace {
            name: format!("Forbidden workspace {index}").try_into()?,
        };
        let payload = RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some(request_id),
            workspace_id: None,
            operation,
        });
        let request = LinkRequest::new(test_id(index * 10 + 2)?, caller.clone(), payload.clone())?;
        let actual = exchange(
            socket,
            DataFrame::Request {
                request: Box::new(request.clone()),
            },
        )
        .await?;
        require(actual, expected_reply(&request, refusal.clone()))?;

        let correlation_id = test_id(index * 10 + 3)?;
        let actual = exchange(
            socket,
            DataFrame::RequestStatus {
                correlation_id: correlation_id.clone(),
                caller: caller.clone(),
                query: RequestStatusQuery {
                    request_id: String::from(test_id(index * 10 + 1)?).parse()?,
                    scope: RequestScope::Organization,
                },
            },
        )
        .await?;
        require(
            actual,
            DataFrame::RequestStatusReply {
                correlation_id,
                result: Box::new(RequestStatusAnswer::Refused(RequestRefusal::Workspace(
                    refusal.clone(),
                ))),
            },
        )?;
        let request = LinkRequest::new(test_id(index * 10 + 4)?, caller, payload)?;
        batch.push((request, refusal));
    }
    let frame = DataFrame::Batch {
        requests: LinkRequestBatch::try_from(
            batch
                .iter()
                .map(|(request, _)| request.clone())
                .collect::<Vec<_>>(),
        )?,
    };
    send(socket, &LinkFrame::Data(frame)).await?;
    for (request, refusal) in batch {
        require(read(socket).await?, expected_reply(&request, refusal))?;
    }
    if store.read(counts()).await? != before {
        return Err("a refused caller wrote to the workspace directory or request log".into());
    }
    Ok(())
}

fn test_id(id: usize) -> Result<CorrelationId> {
    Ok(format!("019f0000-0000-7000-8000-00000000e0{id:02x}").parse()?)
}

fn expected_reply(request: &LinkRequest, refusal: RefusalReason) -> DataFrame {
    DataFrame::Reply {
        correlation_id: request.correlation_id().clone(),
        result: Box::new(ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::Refused(refusal),
            actors: vec![],
        })),
    }
}

async fn exchange(
    socket: &mut reqwest_websocket::WebSocket,
    frame: DataFrame,
) -> Result<DataFrame> {
    send(socket, &LinkFrame::Data(frame)).await?;
    read(socket).await
}

async fn read(socket: &mut reqwest_websocket::WebSocket) -> Result<DataFrame> {
    match receive(socket).await? {
        LinkFrame::Data(frame) => Ok(frame),
        other => Err(format!("expected a data frame, got {other:?}").into()),
    }
}

fn require(actual: DataFrame, expected: DataFrame) -> Result<()> {
    if actual != expected {
        return Err(format!("got {actual:?}, expected {expected:?}").into());
    }
    Ok(())
}
