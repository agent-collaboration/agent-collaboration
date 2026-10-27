//! Fleet lifecycle commands mutate the real stand-in through SDK/MCP admission.
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "This fixture source belongs to the split lane, which fixes its allowances at integration."
)]
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
pub mod board_fixture;
use board_cli_execution::{accepted, cli};
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn initiative_close_and_workspace_archive_reach_the_fleet_owner() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    fixture.project().await?;
    let board = BoardId::generate();
    let actor = serde_json::to_string(&fixture.actor)?;
    let directory = fixture._directory.path();
    accepted(
        &cli(
            directory,
            &[
                "board",
                "create",
                "--workspace-id",
                fixture.workspace.as_str(),
                "--board-id",
                board.as_str(),
                "--name",
                "initiative",
                "--kind",
                r#"{"kind":"initiative","goal":"ship","doneWhen":"accepted"}"#,
                "--actor",
                &actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory,
            &[
                "board",
                "close",
                "--board-id",
                board.as_str(),
                "--actor",
                &actor,
            ],
        )
        .await?,
    );
    let client = fixture.api.client("fleet CLI proof").await?;
    assert_eq!(
        client
            .board_show(BoardShowRequest {
                board_id: board.clone()
            })
            .await?
            .board
            .state,
        BoardState::Closed
    );
    accepted(
        &cli(
            directory,
            &[
                "workspace",
                "archive",
                "--workspace-id",
                fixture.workspace.as_str(),
                "--actor",
                &actor,
            ],
        )
        .await?,
    );
    assert_eq!(
        client
            .workspace_show(WorkspaceShowRequest {
                workspace_id: fixture.workspace.clone()
            })
            .await?
            .workspace
            .state,
        WorkspaceState::Archiving
    );
    assert_eq!(
        client
            .board_show(BoardShowRequest { board_id: board })
            .await?
            .board
            .state,
        BoardState::Archived
    );
    assert_eq!(
        client
            .board_show(BoardShowRequest {
                board_id: fixture.board.clone()
            })
            .await?
            .board
            .state,
        BoardState::Archiving
    );
    let response = fixture
        .api
        .call("workspace_show", json!({"workspaceId":fixture.workspace}))
        .await?;
    assert_eq!(
        response.pointer("/result/workspace/state"),
        Some(&json!("archiving"))
    );
    fixture.stop().await
}
