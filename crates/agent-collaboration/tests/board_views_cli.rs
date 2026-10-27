//! The built CLI must use typed SDK calls to the native board view tools.
#[path = "support/board_cli_execution.rs"]
mod board_cli_execution;
#[path = "../../collaboration-service/tests/support/board_fixture.rs"]
#[expect(
    dead_code,
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_cli_execution::{accepted, cli, cli_with_session};
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn view_commands_reach_the_real_service_and_report_read_marks() -> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let directory = fixture._directory.path();
    let work = cli(
        directory,
        &["view", "work", "--project-id", project.as_str()],
    )
    .await?;
    assert_eq!(
        work.pointer("/result/record/readyQueue"),
        Some(&json!([task])),
        "{work}"
    );
    let actor = serde_json::to_string(&fixture.actor)?;
    accepted(
        &cli(
            directory,
            &[
                "workstream",
                "rename",
                "--project-id",
                project.as_str(),
                "--workstream-id",
                workstream.as_str(),
                "--name",
                "CLI named lane",
                "--expected-revision",
                "0",
                "--actor",
                &actor,
            ],
        )
        .await?,
    );
    let subject = serde_json::to_string(&TrailSubject::Workstream(workstream))?;
    let card = cli(
        directory,
        &[
            "view",
            "card",
            "--project-id",
            project.as_str(),
            "--actor",
            &actor,
            "--subject",
            &subject,
        ],
    )
    .await?;
    assert_eq!(
        card.pointer("/result/record/assignment/kind"),
        Some(&json!("lead"))
    );
    assert_eq!(card.pointer("/result/record/readMark"), None);
    let owner = cli(directory, &["view", "owner"]).await?;
    assert_eq!(owner.pointer("/result/record/waiting"), Some(&json!([])));
    assert_eq!(owner.pointer("/result/record/readMarks"), None);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("store missing")?;
    let mut snapshot = store.snapshot().await?;
    let lead = snapshot
        .records(RecordKind::RoleAssignment)
        .await?
        .into_iter()
        .find_map(|record| match record {
            Record::RoleAssignment(role) if role.role() == AuthorityRole::Lead => Some(role.holder),
            _ => None,
        })
        .ok_or("lead missing")?;
    snapshot.finish().await?;
    let lead = serde_json::to_string(&lead)?;
    let load = cli(directory, &["view", "lead-load", "--lead", &lead]).await?;
    assert_eq!(load.pointer("/result/record/owed"), Some(&json!([])));
    let own = cli_with_session(
        directory,
        &["view", "session-context", "--session", "self"],
        Some("fixture-current-session"),
    )
    .await?;
    assert_eq!(
        own.pointer("/result/record/payload/session/endpoint/serviceId"),
        Some(&json!("00000000-0000-4000-8000-000000000001"))
    );
    assert_eq!(
        own.pointer("/result/record/payload/session/sessionId"),
        Some(&json!("fixture-current-session"))
    );
    let session=json!({"endpoint":{"endpointId":"codex-local","serviceId":"00000000-0000-4000-8000-000000000001"},"sessionId":"00000000-0000-4000-8000-000000000003"}).to_string();
    let context = cli(
        directory,
        &["view", "session-context", "--session", &session],
    )
    .await?;
    assert_eq!(
        context.pointer("/result/record/kind"),
        Some(&json!("context"))
    );
    fixture.stop().await
}
