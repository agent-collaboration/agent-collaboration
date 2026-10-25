//! The executable reads receipts through the SDK and the real served API.
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[allow(dead_code)] // This case uses the Fleet-only portion of the shared fixture.
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
mod board_fixture;
use board_cli_execution::{accepted, cli};
use board_fixture::{BoardFixture, TestError};
use message_board::{ProjectId, RequestId};
use serde_json::json;
use std::process::Command;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove results; Result propagates fixture failures.
async fn request_effects_cli_reads_a_decision_and_transmits_an_after_argument()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_replay().await?;
    let request = RequestId::generate();
    let actor = serde_json::to_string(&fixture.actor)?;
    accepted(
        &cli(
            fixture._directory.path(),
            &[
                "workspace",
                "create",
                "--workspace-id",
                fixture.workspace.as_str(),
                "--name",
                "receipt CLI",
                "--actor",
                &actor,
                "--request-id",
                request.as_str(),
            ],
        )
        .await?,
    );
    let result = cli(
        fixture._directory.path(),
        &[
            "request-effects",
            request.as_str(),
            "--workspace-id",
            fixture.workspace.as_str(),
        ],
    )
    .await?;
    assert_eq!(
        result.pointer("/result/record/kind"),
        Some(&json!("decision")),
        "{result}"
    );
    assert_eq!(
        result.pointer("/result/record/decision/kind"),
        Some(&json!("accepted")),
        "{result}"
    );
    assert_eq!(
        result.pointer("/result/record/decision/effects/totalCount"),
        Some(&json!(1))
    );
    let invalid = cli(
        fixture._directory.path(),
        &[
            "request-effects",
            request.as_str(),
            "--workspace-id",
            fixture.workspace.as_str(),
            "--after",
            "opaque-but-invalid",
        ],
    )
    .await?;
    assert_eq!(
        invalid.pointer("/result/record/kind"),
        Some(&json!("refused")),
        "{invalid}"
    );
    assert_eq!(
        invalid.pointer("/result/record/reason/kind"),
        Some(&json!("cursorInvalid")),
        "{invalid}"
    );
    fixture.stop().await?;
    Ok(())
}

#[test]
fn request_effects_cli_requires_exactly_one_explicit_scope() {
    let request = RequestId::generate();
    let project = ProjectId::generate();
    for selectors in [
        vec![],
        vec![
            "--project-id",
            project.as_str(),
            "--workspace-id",
            project.as_str(),
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(["request-effects", request.as_str()])
            .args(selectors)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
    }
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // The real MAC verifier is the continuation oracle.
async fn request_effects_cli_resumes_a_signed_page_without_normalizing_the_token()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    for index in 0..257 {
        let mut create = fixture.header();
        create["workspaceId"] = json!(fixture.workspace);
        create["boardId"] = json!(message_board::BoardId::generate());
        create["name"] = json!(format!("CLI receipt board {index}"));
        create["description"] = json!("");
        create["kind"] = json!({"kind":"area"});
        board_fixture::accepted(&fixture.api.call("board_create", create).await?);
    }
    let actor = serde_json::to_string(&fixture.actor)?;
    let request = RequestId::generate();
    let written = cli(
        fixture._directory.path(),
        &[
            "workspace",
            "archive",
            "--workspace-id",
            fixture.workspace.as_str(),
            "--request-id",
            request.as_str(),
            "--actor",
            &actor,
        ],
    )
    .await?;
    accepted(&written);
    let prefix = written
        .pointer("/result/record/result/decision/effects")
        .ok_or("first page")?;
    assert_eq!(prefix["totalCount"], json!(259));
    assert_eq!(prefix["entries"].as_array().ok_or("entries")?.len(), 256);
    let token = prefix["next"].as_str().ok_or("signed next")?;
    let resumed = cli(
        fixture._directory.path(),
        &[
            "request-effects",
            request.as_str(),
            "--workspace-id",
            fixture.workspace.as_str(),
            "--after",
            token,
        ],
    )
    .await?;
    let page = resumed
        .pointer("/result/record/decision/effects")
        .ok_or("resumed page")?;
    assert_eq!(page["entries"].as_array().ok_or("entries")?.len(), 3);
    assert_eq!(page["totalCount"], prefix["totalCount"]);
    assert_eq!(page["digest"], prefix["digest"]);
    assert!(page["next"].is_null());
    let wrapped = format!(" {token} ");
    let refused = cli(
        fixture._directory.path(),
        &[
            "request-effects",
            request.as_str(),
            "--workspace-id",
            fixture.workspace.as_str(),
            "--after",
            &wrapped,
        ],
    )
    .await?;
    assert_eq!(
        refused.pointer("/result/record/kind"),
        Some(&json!("refused")),
        "{refused}"
    );
    assert_eq!(
        refused.pointer("/result/record/reason/kind"),
        Some(&json!("cursorInvalid")),
        "the CLI normalized an invalid signed-token spelling: {refused}"
    );
    fixture.stop().await?;
    Ok(())
}
