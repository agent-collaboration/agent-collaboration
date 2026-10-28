use collaboration_client::{BoardClientError, CollaborationClient};
use message_board::{
    ActingForIdentity, Description, HumanId, Identity, ResourceIdentity, ResourceName,
    WorkspaceCreateRequest, WorkspaceId,
};
use serde_json::Value;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/scripted_api.rs"]
mod scripted_api;
use scripted_api::ScriptedApi;

#[tokio::test]
async fn response_loss_after_board_write_transmission_returns_typed_inspection_identity() {
    let workspace_id = WorkspaceId::generate();
    let (api, mut calls) = ScriptedApi::start().unwrap();
    let client = CollaborationClient::connect(api.directory(), "board-uncertainty-test", "1")
        .await
        .unwrap();
    let expected_workspace_id = workspace_id.clone();
    let server = tokio::spawn(async move {
        let request = calls.next().await.unwrap();
        assert_eq!(request.tool, "workspace_create");
        assert_eq!(
            request
                .arguments
                .pointer("/workspaceId")
                .and_then(Value::as_str),
            Some(expected_workspace_id.as_str())
        );
        // The complete write was received. Close without replying.
        request.lose_response();
        calls
    });

    let error = client
        .workspace_create(project_create_request(workspace_id.clone()))
        .await
        .unwrap_err();
    let mut calls = server.await.unwrap();

    match error {
        BoardClientError::OutcomeUnknown {
            resource,
            message,
            next_action,
        } => {
            assert_eq!(resource, ResourceIdentity::Workspace { workspace_id });
            assert_eq!(next_action, message_board::BoardNextAction::InspectResource);
            assert!(message.contains("Inspect"));
        }
        other => panic!("expected typed uncertain mutation outcome, got {other:?}"),
    }
    // The uncertain write is never replayed.
    assert!(
        calls
            .none_within(std::time::Duration::from_millis(100))
            .await
    );
}

#[expect(
    clippy::unwrap_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]
fn project_create_request(workspace_id: WorkspaceId) -> WorkspaceCreateRequest {
    WorkspaceCreateRequest {
        request_id: message_board::RequestId::generate(),
        protocol_version: message_board::ProtocolVersion::CURRENT,
        workspace_id,
        name: ResourceName::try_from("Uncertain SDK write".to_owned()).unwrap(),
        description: Description::try_from(String::new()).unwrap(),
        actor: Identity::Human {
            human_id: HumanId::try_from("sdk-uncertainty".to_owned()).unwrap(),
        },
        acting_for: Some(ActingForIdentity::Human {
            human_id: HumanId::try_from("owner".to_owned()).unwrap(),
        }),
    }
}
