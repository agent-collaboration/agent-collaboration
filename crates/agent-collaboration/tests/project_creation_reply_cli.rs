//! Actual CLI exit codes and stable project identity through accepted-pending and local creation.

#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;

#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "Shared test fixture helpers serve different callers."
)]
pub mod board_fixture;
use board_cli_execution::{accepted, cli};
use board_fixture::{BoardFixture, TestError};
use message_board::{ProjectId, RequestId};
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn accepted_pending_project_create_is_exit_zero_then_local_and_keyed_replay()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let request_id = RequestId::generate();
    let mut creation = fixture.header();
    creation["requestId"] = json!(request_id);
    creation["projectId"] = json!(project);
    creation["boardId"] = json!(fixture.board);
    creation["title"] = json!("durable accepted CLI creation");
    creation["description"] = json!("preserve initial text");
    let request = serde_json::from_value(creation)?;
    let args = [
        "project",
        "create",
        "--request-id",
        request_id.as_str(),
        "--project-id",
        project.as_str(),
        "--board-id",
        fixture.board.as_str(),
        "--title",
        "durable accepted CLI creation",
        "--description",
        "preserve initial text",
        "--actor",
        r#"{"kind":"human","humanId":"owner"}"#,
    ];
    // The helper asserts actual process exit0, not only a decoded result shape.
    // The fixture's real central durably decides this first public request.
    let pending = cli(fixture._directory.path(), &args).await?;
    assert_eq!(
        pending.pointer("/result/record/result/kind"),
        Some(&json!("projectCreationPending")),
        "{pending}"
    );
    assert_eq!(
        pending.pointer("/result/record/result/decision/project"),
        Some(&json!(project))
    );
    assert_eq!(cli(fixture._directory.path(), &args).await?, pending);
    // A controlled native-grant input supplies Ready for this CLI-layer case;
    // the Host companion proves the real session/registration interaction.
    fixture.prepare_registered_creation(&request).await?;
    let created = cli(fixture._directory.path(), &args).await?;
    accepted(&created);
    let replay = cli(fixture._directory.path(), &args).await?;
    assert_eq!(
        replay["result"]["record"]["result"],
        created["result"]["record"]["result"]
    );
    fixture.stop().await
}
