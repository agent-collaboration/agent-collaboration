//! A live admission read can fail independently for one batch member.
use super::fixture::{ConnectedLink, Fixture, receive, send, session_caller};
use central_link_protocol::{
    DataFrame, LinkRequest, LinkRequestBatch, ReplyPayload, RequestPayload,
};
use central_storage::{SqlValue, Statement, StoreFailure};
use collaboration_central_conformance::ConformanceResult as Result;
use workspace_model::{
    WorkspaceAnswer, WorkspaceOperation, WorkspaceReply, WorkspaceRequest, WorkspaceResult,
};

#[tokio::test]
async fn batch_continues_after_a_transient_middle_admission_failure() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = transient_admission_faults(&fixture).await;
    fixture.close().await?;
    result
}

async fn transient_admission_faults(fixture: &Fixture) -> Result<()> {
    let mut link = ConnectedLink::open(fixture).await?;
    let socket = &mut link.socket;
    let runtime = fixture.runtime();
    for (round, failure) in [StoreFailure::Unavailable, StoreFailure::AnswerLost]
        .into_iter()
        .enumerate()
    {
        let requests = (0..3)
            .map(|index| request(round, index))
            .collect::<Result<Vec<_>>>()?;
        runtime.fail_machine_restore_after(1, failure).await?;
        let frame = DataFrame::Batch {
            requests: LinkRequestBatch::new(requests.clone())?,
        };
        send(socket, &frame).await?;
        for (index, request) in requests.iter().enumerate() {
            let reply = receive(socket).await?;
            let DataFrame::Reply {
                correlation_id,
                result,
            } = reply
            else {
                return Err(
                    format!("batch member {index} must receive a Reply, got {reply:?}").into(),
                );
            };
            if &correlation_id != request.correlation_id() {
                return Err(
                    format!("batch member {index} received a different correlation").into(),
                );
            }
            let expected_name = name(round, index);
            match (index, *result) {
                (
                    1,
                    ReplyPayload::Workspace(WorkspaceReply {
                        result: WorkspaceResult::CentralUnavailable,
                        ..
                    }),
                ) => {}
                (
                    0 | 2,
                    ReplyPayload::Workspace(WorkspaceReply {
                        result:
                            WorkspaceResult::Done(WorkspaceAnswer::WorkspaceCreated { workspace }),
                        ..
                    }),
                ) if workspace.name.as_str() == expected_name => {}
                (_, actual) => {
                    return Err(format!("unexpected batch result for {index}: {actual:?}").into());
                }
            }
        }
    }
    let rows = runtime
        .read(Statement {
            sql: "SELECT name FROM directory_workspaces ORDER BY name",
            args: vec![],
        })
        .await?;
    let expected = [(0, 0), (0, 2), (1, 0), (1, 2)]
        .map(|(round, index)| vec![SqlValue::Text(name(round, index))]);
    if rows.rows != expected {
        return Err(format!(
            "the batch persisted incorrect directory rows: {:?}",
            rows.rows
        )
        .into());
    }
    Ok(())
}

fn name(round: usize, index: usize) -> String {
    format!("Transient restore {round} {index}")
}

fn request(round: usize, index: usize) -> Result<LinkRequest> {
    let id = round * 8 + index * 2;
    Ok(LinkRequest::new(
        format!("019f0000-0000-7000-8000-00000000e4{id:02x}").parse()?,
        session_caller()?,
        RequestPayload::Workspace(WorkspaceRequest {
            request_id: Some(format!("019f0000-0000-7000-8000-00000000e4{:02x}", id + 1).parse()?),
            workspace_id: None,
            operation: WorkspaceOperation::CreateWorkspace {
                name: name(round, index).try_into()?,
            },
        }),
    )?)
}
