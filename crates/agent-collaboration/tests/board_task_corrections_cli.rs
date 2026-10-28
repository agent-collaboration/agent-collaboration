//! CLI drop and removal flags reach real MCP admission and captured project records.
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
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn drop_reason_and_removed_list_flags_reach_the_real_board() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let subtask = SubtaskId::generate();
    let directory = fixture._directory.path();
    let actor = r#"{"kind":"human","humanId":"owner"}"#;
    accepted(
        &cli(
            directory,
            &[
                "subtask",
                "add",
                "--project-id",
                project.as_str(),
                "--task-id",
                task.as_str(),
                "--subtask-id",
                subtask.as_str(),
                "--text",
                "CLI checklist",
                "--place",
                "80",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let reason = "CLI caller chose to retire this task";
    accepted(
        &cli(
            directory,
            &[
                "task",
                "set-status",
                "--project-id",
                project.as_str(),
                "--task-id",
                task.as_str(),
                "--status",
                "drop",
                "--reason",
                reason,
                "--expected-revision",
                "0",
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    accepted(
        &cli(
            directory,
            &[
                "subtask",
                "remove",
                "--project-id",
                project.as_str(),
                "--subtask-id",
                subtask.as_str(),
                "--actor",
                actor,
            ],
        )
        .await?,
    );
    let removed = cli(
        directory,
        &["subtask", "list", "--project-id", project.as_str()],
    )
    .await?;
    assert_eq!(
        removed.pointer("/result/page/records"),
        Some(&json!([])),
        "{removed}"
    );
    let included = cli(
        directory,
        &[
            "subtask",
            "list",
            "--project-id",
            project.as_str(),
            "--include-removed",
        ],
    )
    .await?;
    assert_eq!(
        included.pointer("/result/page/records/0/record/removed"),
        Some(&json!(true)),
        "{included}"
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(read.records(RecordKind::Fact).await?.iter().any(|record| matches!(record,
        Record::Fact(Fact { kind: FactKind::Lifecycle(LifecycleFact::Dropped { task: subject, reason: text }), .. })
        if subject == &task && text == reason)));
    assert!(
        matches!(read.record(&subtask.into()).await?, Some(Record::Subtask(row)) if row.removed)
    );
    read.finish().await?;
    fixture.stop().await
}
