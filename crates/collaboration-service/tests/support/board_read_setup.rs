//! Real typed board read setup shared by retained behavioral scenarios.
#![allow(dead_code)] // Each integration target uses its own subset of setup operations.
use super::board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

pub async fn keyed_call(
    fixture: &BoardFixture,
    tool: &str,
    fields: Value,
) -> Result<Value, TestError> {
    let mut request = fixture.header();
    request
        .as_object_mut()
        .ok_or("request")?
        .extend(fields.as_object().ok_or("fields")?.clone());
    Ok(fixture.api.call(tool, request).await?)
}

pub async fn create_location(
    fixture: &BoardFixture,
    label: &str,
) -> Result<(WorkspaceId, BoardId, ProjectId), TestError> {
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    for (tool, fields) in [
        (
            "workspace_create",
            json!({"workspaceId":workspace,"name":label,"description":"workspace description"}),
        ),
        (
            "board_create",
            json!({"workspaceId":workspace,"boardId":board,"name":label,"description":"board description","kind":{"kind":"area"}}),
        ),
        (
            "project_create",
            json!({"boardId":board,"projectId":project,"title":label}),
        ),
    ] {
        let mut request = fixture.header();
        request
            .as_object_mut()
            .ok_or("request")?
            .extend(fields.as_object().ok_or("fields")?.clone());
        accepted(&fixture.api.call(tool, request).await?);
    }
    Ok((workspace, board, project))
}
pub async fn post_at_placement(
    fixture: &BoardFixture,
    placement: Placement,
    fields: Value,
) -> Result<PostId, TestError> {
    let id = PostId::generate();
    let mut request = fixture.header();
    let (tool, content) = match placement {
        Placement::Project { project_id } => (
            "thread_create",
            json!({"postId":id,"projectId":project_id,"about":fixture.discussion_subject(&project_id).await?,"role":null,"watch":false,"references":[],"addressees":[]}),
        ),
        Placement::Thread { thread_id } => (
            "thread_post",
            json!({"postId":id,"placement":PostTarget::Thread { thread: thread_id.into() },"references":[],"addressees":[]}),
        ),
    };
    request
        .as_object_mut()
        .ok_or("request")?
        .extend(content.as_object().ok_or("content")?.clone());
    request
        .as_object_mut()
        .ok_or("request")?
        .extend(fields.as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call(tool, request).await?);
    Ok(id)
}

pub fn inbox_request(
    scope: InboxScope,
    reader: Identity,
    mode: InboxReadMode,
) -> Result<InboxFetchRequest, TestError> {
    Ok(InboxFetchRequest::new(InboxFetchRequestProps {
        protocol_version: ProtocolVersion::CURRENT,
        acting_for: None,
        reader,
        scope,
        read_mode: mode,
        initializations: Vec::new(),
        page: PageRequest::default(),
    })?)
}
pub fn initialized_request(
    project: ProjectId,
    reader: Identity,
) -> Result<InboxFetchRequest, TestError> {
    Ok(InboxFetchRequest::new(InboxFetchRequestProps {
        protocol_version: ProtocolVersion::CURRENT,
        acting_for: None,
        reader,
        scope: InboxScope::Project {
            project_id: project.clone(),
        },
        read_mode: InboxReadMode::Unread,
        initializations: vec![ReadInitializationEntry {
            project_id: project,
            request_id: RequestId::generate(),
            observed_epoch: Some(EpochNumber::try_from(1)?),
            resubmits: None,
        }],
        page: PageRequest::default(),
    })?)
}
pub fn human_reader(name: &str) -> Result<Identity, TestError> {
    Ok(Identity::Human {
        human_id: HumanId::try_from(name.to_owned())?,
    })
}
pub fn inbox_post_ids(records: &[InboxActivity]) -> Vec<PostId> {
    records
        .iter()
        .filter_map(|record| match record {
            InboxActivity::PostCreated { post, .. } => Some(post.post_id.clone()),
            InboxActivity::ThreadStateChanged { .. } => None,
        })
        .collect()
}
