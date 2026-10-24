//! Public SDK request construction and real board fixture setup.
use super::board_fixture::{BoardFixture, TestError};
use collaboration_client::CollaborationClient;
use collaboration_client::board::{RequestDecision, RequestResult, WriteReply};
use message_board::*;
use serde_json::{Value, json};

pub(super) fn session(session_id: String) -> Result<Identity, TestError> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from(session_id)?,
        },
    })
}
pub(super) fn request<TRequest: serde::de::DeserializeOwned>(
    actor: &Identity,
    fields: Value,
) -> Result<TRequest, TestError> {
    let mut value = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":actor});
    value
        .as_object_mut()
        .ok_or("request object")?
        .extend(fields.as_object().ok_or("request fields")?.clone());
    Ok(serde_json::from_value(value)?)
}
pub(super) fn accepted(reply: &WriteReply) {
    assert!(
        matches!(
            &reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}
pub(super) fn refused(reply: &WriteReply) -> Result<&[Refusal], TestError> {
    match &reply.result {
        RequestResult::Decided(RequestDecision::Rejected { reasons }) => Ok(reasons),
        _ => Err(format!("expected recorded rejection: {reply:?}").into()),
    }
}
pub(super) async fn start() -> Result<(BoardFixture, CollaborationClient, ProjectId), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("board-control-path").await?;
    let actor: Identity = serde_json::from_value(fixture.actor.clone())?;
    let project = ProjectId::generate();
    let create = request::<ProjectCreateRequest>(
        &actor,
        json!({"projectId":project,"boardId":fixture.board,"title":"Project"}),
    )?;
    fixture.prepare_registered_creation(&create).await?;
    accepted(&client.project_create(create).await?);
    Ok((fixture, client, project))
}
pub(super) async fn post(
    fixture: &BoardFixture,
    client: &CollaborationClient,
    actor: &Identity,
    placement: Placement,
    text: &str,
    references: Vec<ReferenceTarget>,
) -> Result<Post, TestError> {
    let id = PostId::generate();
    match placement {
        Placement::Project { project_id } => accepted(&client.thread_create(request(actor, json!({"postId":id,"projectId":project_id,"about":fixture.discussion_subject(&project_id).await?,"role":null,"watch":false,"text":text,"references":references,"addressees":[]}))?).await?),
        Placement::Thread { thread_id } => accepted(&client.thread_post(request(actor,json!({"postId":id,"placement":PostTarget::Thread { thread: thread_id.into() },"text":text,"references":references,"addressees":[]}))?).await?),
    }
    Ok(client.post_show(PostShowRequest { post_id: id }).await?.0)
}
pub(super) fn inbox_request(
    reader: &Identity,
    project: &ProjectId,
    mode: InboxReadMode,
) -> Result<InboxFetchRequest, TestError> {
    Ok(InboxFetchRequest::new(InboxFetchRequestProps {
        protocol_version: ProtocolVersion::CURRENT,
        acting_for: None,
        reader: reader.clone(),
        scope: InboxScope::Project {
            project_id: project.clone(),
        },
        read_mode: mode,
        initializations: Vec::new(),
        page: PageRequest::default(),
    })?)
}
pub(super) fn cursor(post: &Post) -> Result<FeedCursor, TestError> {
    match &post.position {
        RecordPlacement::Project(entry) => Ok(entry.cursor),
        _ => Err("project post has fleet position".into()),
    }
}
#[allow(clippy::panic_in_result_fn)] // This test helper checks the returned independent reply.
pub(super) async fn acknowledge(
    client: &CollaborationClient,
    actor: &Identity,
    scope: ReadScope,
    through: FeedCursor,
) -> Result<(), TestError> {
    let result = client
        .inbox_acknowledge(InboxAcknowledgeRequest::new(
            InboxAcknowledgeRequestProps {
                protocol_version: ProtocolVersion::CURRENT,
                actor: actor.clone(),
                acting_for: None,
                entries: vec![AcknowledgementEntry {
                    request_id: RequestId::generate(),
                    scope: scope.into(),
                    through,
                }],
            },
        )?)
        .await?;
    assert_eq!(result.outcomes.len(), 1);
    let ProjectWriteCallResult::Replied { reply } = &result
        .outcomes
        .first()
        .ok_or("missing acknowledgement outcome")?
        .outcome
    else {
        return Err("acknowledgement failed".into());
    };
    accepted(reply);
    Ok(())
}
