//! Typed requests for the route fixture; creation runs through its real enrolled Host.
use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use message_board::{
    BoardCreateRequest, BoardId, BoardKind, Description, HumanId, Identity, ProjectCreateRequest,
    ProjectId, ProtocolVersion, RequestId, ResourceName, WorkspaceCreateRequest, WorkspaceId,
};

pub(super) struct RegisteredRouteProject {
    pub(super) workspace: WorkspaceCreateRequest,
    pub(super) board: BoardCreateRequest,
    pub(super) project: ProjectCreateRequest,
}

pub(super) fn route_project_requests() -> RegisteredRouteProject {
    let owner = Identity::Human {
        human_id: HumanId::try_from("route-test-owner".to_owned()).expect("owner"),
    };
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let project = ProjectId::generate();
    RegisteredRouteProject {
        workspace: WorkspaceCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            workspace_id: workspace.clone(),
            name: ResourceName::try_from("Route workspace".to_owned()).expect("workspace"),
            description: Description::try_from(String::new()).expect("description"),
            actor: owner.clone(),
            acting_for: None,
        },
        board: BoardCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            board_id: board.clone(),
            workspace_id: workspace,
            name: ResourceName::try_from("Route board".to_owned()).expect("board"),
            description: Description::try_from(String::new()).expect("description"),
            kind: BoardKind::Area {},
            actor: owner.clone(),
            acting_for: None,
        },
        project: ProjectCreateRequest {
            request_id: RequestId::generate(),
            protocol_version: ProtocolVersion::CURRENT,
            project_id: project,
            board_id: board,
            title: ResourceName::try_from("Route project".to_owned()).expect("project"),
            description: None,
            actor: owner,
            acting_for: None,
        },
    }
}

pub(super) fn assert_board_accepted(reply: &WriteReply) {
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "board fixture refused: {reply:?}"
    );
}

/// Preserve the same key while the real session or initial registration is explicitly pending.
pub(super) async fn settled_creation<Call, Reply>(mut call: Call) -> WriteReply
where
    Call: FnMut() -> Reply,
    Reply: std::future::Future<Output = Result<WriteReply, collaboration_client::BoardClientError>>,
{
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            match call().await {
                Ok(reply) if matches!(reply.result, RequestResult::PendingUnreachable) => {}
                Err(collaboration_client::BoardClientError::Rejected(failure))
                    if failure.kind == message_board::BoardFailureKind::BoardUnavailable => {}
                result => return result.expect("public route creation"),
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("route creation settled")
}
