//! Description creation and revisioned file requests traverse the actual CLI and native API.
#![allow(clippy::panic_in_result_fn)]
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
pub mod board_fixture;
use board_cli_execution::{accepted, cli};
use board_fixture::{BoardFixture, TestError};
use message_board::{ProjectId, RequestId};
use serde_json::json;

#[tokio::test]
async fn project_descriptions_keep_exact_text_through_create_describe_show_and_retry()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = ProjectId::generate();
    let actor = r#"{"kind":"human","humanId":"owner"}"#;
    let original = " CLI description\n\t ";
    let request_id = RequestId::generate();
    let mut creation = fixture.header();
    creation["requestId"] = json!(request_id);
    creation["projectId"] = json!(project);
    creation["boardId"] = json!(fixture.board);
    creation["title"] = json!("CLI description");
    creation["description"] = json!(original);
    creation["actor"] = serde_json::from_str(actor)?;
    fixture
        .prepare_registered_creation(&serde_json::from_value(creation)?)
        .await?;
    accepted(
        &cli(
            fixture._directory.path(),
            &[
                "project",
                "create",
                "--request-id",
                request_id.as_str(),
                "--project-id",
                project.as_str(),
                "--board-id",
                fixture.board.as_str(),
                "--title",
                "CLI description",
                "--description",
                original,
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let shown = cli(
        fixture._directory.path(),
        &["project", "show", "--project-id", project.as_str()],
    )
    .await?;
    assert_eq!(
        shown.pointer("/result/record/state/kind"),
        Some(&json!("local"))
    );
    assert_eq!(
        shown.pointer("/result/record/state/value/description"),
        Some(&json!(original))
    );
    assert!(
        shown
            .pointer("/result/record/project/description")
            .is_none()
    );
    let revision = shown
        .pointer("/result/record/state/value/revision")
        .and_then(serde_json::Value::as_u64)
        .ok_or("project revision")?;
    let updated = " \nfile description\0\u{0001}\t ";
    let request = json!({
        "requestId":RequestId::generate(),"protocolVersion":1,
        "projectId":project,"actor":{"kind":"human","humanId":"owner"},
        "actingFor":null,
        "operation":{"kind":"projectDescribe","description":updated,"expectedRevision":revision}
    });
    let path = fixture._directory.path().join("description-request.json");
    std::fs::write(&path, serde_json::to_vec(&request)?)?;
    let arguments = [
        "project",
        "describe",
        "--request-file",
        path.to_str().ok_or("request path")?,
    ];
    let first = cli(fixture._directory.path(), &arguments).await?;
    accepted(&first);
    let replay = cli(fixture._directory.path(), &arguments).await?;
    assert_eq!(
        replay.pointer("/result/record/result"),
        first.pointer("/result/record/result")
    );
    let shown = cli(
        fixture._directory.path(),
        &["project", "show", "--project-id", project.as_str()],
    )
    .await?;
    assert_eq!(
        shown.pointer("/result/record/state/value/description"),
        Some(&json!(updated))
    );
    assert_eq!(
        shown.pointer("/result/record/state/value/revision"),
        Some(&json!(revision + 1))
    );
    fixture.stop().await
}
