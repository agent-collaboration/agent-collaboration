use crate::e2e_support::{
    CentralHarness, CliImportFixture, E2eResult, SourceSnapshot, feed_rows, post_rows,
    project_state_rows, published_snapshot, read_report, run_cli,
};
use codex_router_test_support::scenario_finalization::run_scenario;
use serde_json::json;
use std::path::Path;

const PROJECTS: [&str; 3] = [
    "019a0000-0000-7000-8000-000000000015",
    "019a0000-0000-7000-8000-000000000016",
    "019a0000-0000-7000-8000-000000000017",
];

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn actual_cli_replay_ignores_relabel_but_conflicts_on_changed_description() -> E2eResult {
    let fixture = CliImportFixture::new("legacy-complete.sqlite")?;
    let source = SourceSnapshot::capture(&fixture.source)?;
    let identity = std::fs::read(&fixture.identity_file)?;
    let central = CentralHarness::start().await?;

    run_scenario(
        central,
        async |central| {
            let first_url = central.base_url()?.to_owned();
            let first = run_cli(
                &fixture.source,
                &fixture.router_root,
                &fixture.report,
                Some(&first_url),
                "first-importer",
                "original source",
            )
            .await?;
            assert_cli_answer(&first, &fixture.report, 3, "imported")?;
            source.assert_unchanged(&fixture.source)?;
            central.stop_owner().await?;
            let published = snapshots(central.root()).await?;

            let replay_url = central.reopen_owner().await?;
            assert_eq!(replay_url, first_url);
            let replay = run_cli(
                &fixture.source,
                &fixture.router_root,
                &fixture.report,
                Some(&replay_url),
                "second-importer",
                "second source",
            )
            .await?;
            assert_cli_answer(&replay, &fixture.report, 3, "alreadyImported")?;
            source.assert_unchanged(&fixture.source)?;
            central.stop_owner().await?;
            assert_eq!(snapshots(central.root()).await?, published);

            change_topic_description(&fixture.source)?;
            let changed = SourceSnapshot::capture(&fixture.source)?;
            let conflict_url = central.reopen_owner().await?;
            let conflict = run_cli(
                &fixture.source,
                &fixture.router_root,
                &fixture.report,
                Some(&conflict_url),
                "first-importer",
                "original source",
            )
            .await?;
            assert_eq!(conflict.status.code(), Some(2));
            let report = read_report(&fixture.report)?;
            assert_eq!(
                report.get("kind").and_then(serde_json::Value::as_str),
                Some("refused")
            );
            assert!(
                report
                    .get("reason")
                    .and_then(serde_json::Value::as_str)
                    .ok_or("conflict reason missing")?
                    .contains("ImportConflict")
            );
            changed.assert_unchanged(&fixture.source)?;
            central.stop_owner().await?;
            assert_eq!(snapshots(central.root()).await?, published);
            assert_eq!(std::fs::read(&fixture.identity_file)?, identity);
            Ok(())
        },
        CentralHarness::shutdown,
        || Ok(()),
    )
    .await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn actual_cli_publishes_a_zero_post_project_with_only_its_project_state() -> E2eResult {
    let fixture = CliImportFixture::new("legacy-complete.sqlite")?;
    set_description_variants(&fixture.source)?;
    insert_empty_topic(&fixture.source)?;
    let source = SourceSnapshot::capture(&fixture.source)?;
    let central = CentralHarness::start().await?;

    run_scenario(
        central,
        async |central| {
            let endpoint = central.base_url()?.to_owned();
            let output = run_cli(
                &fixture.source,
                &fixture.router_root,
                &fixture.report,
                Some(&endpoint),
                "empty-project-owner",
                "empty project source",
            )
            .await?;
            assert_cli_answer(&output, &fixture.report, 4, "imported")?;
            source.assert_unchanged(&fixture.source)?;
            central.stop_owner().await?;
            assert_project_description(
                central.root(),
                "019a0000-0000-7000-8000-000000000015",
                "",
                "019a0000-0000-7000-8000-000000000001",
                "019a0000-0000-7000-8000-00000000000b",
            )
            .await?;
            assert_project_description(
                central.root(),
                "019a0000-0000-7000-8000-000000000016",
                "  first\r\nsecond\0\tline\n ",
                "019a0000-0000-7000-8000-000000000001",
                "019a0000-0000-7000-8000-00000000000b",
            )
            .await?;
            assert_project_description(
                central.root(),
                "019a0000-0000-7000-8000-000000000017",
                &"λ\0\n".repeat(4096),
                "019a0000-0000-7000-8000-000000000002",
                "019a0000-0000-7000-8000-00000000000c",
            )
            .await?;
            let project = "019a0000-0000-7000-8000-000000000019";
            assert_eq!(
                project_state_rows(central.root(), project).await?,
                vec![json!([
                    project,
                    "A project without posts or activity",
                    "019a0000-0000-7000-8000-000000000001",
                    "019a0000-0000-7000-8000-00000000000b",
                    1,
                    1,
                    null,
                    "legacy_unknown"
                ])]
            );
            assert_eq!(
                feed_rows(central.root(), project).await?,
                vec![json!([1, 1, "project", project, null, "legacy_unknown"])]
            );
            assert!(post_rows(central.root(), project).await?.is_empty());
            let summary_rows = crate::upload_support::read(
                central.root(),
                &format!("copy-{project}"),
                "SELECT record AS value FROM _copy_summary",
            )
            .await
            .map_err(crate::e2e_support::promote_error)?;
            let summary: collaboration_central_core::CopyDirectorySummary =
                serde_json::from_str(summary_rows.first().ok_or("copy summary missing")?)?;
            assert_eq!(
                summary.summary,
                board_sync_protocol::BoardAnnotation::ProjectSummary {
                    latest_record_at: message_board::RecordTime::LegacyUnknown,
                }
            );
            Ok(())
        },
        CentralHarness::shutdown,
        || Ok(()),
    )
    .await
}

#[derive(Debug, Eq, PartialEq)]
struct PublishedState {
    copies: Vec<Vec<String>>,
    directory: Vec<String>,
}

async fn snapshots(root: &Path) -> E2eResult<PublishedState> {
    let mut copy_snapshots = Vec::new();
    for project in PROJECTS {
        copy_snapshots.push(published_snapshot(root, project).await?);
    }
    let projection = crate::upload_support::projection(root, PROJECTS.len())
        .await
        .map_err(crate::e2e_support::promote_error)?;
    Ok(PublishedState {
        copies: copy_snapshots,
        directory: projection,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_cli_answer(
    output: &std::process::Output,
    report_path: &Path,
    count: usize,
    expected_answer: &str,
) -> E2eResult {
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_report(report_path)?;
    assert_eq!(
        report.get("kind").and_then(serde_json::Value::as_str),
        Some("uploaded")
    );
    let projects = report
        .get("publication")
        .and_then(|publication| publication.get("projects"))
        .and_then(serde_json::Value::as_array)
        .ok_or("publication results missing")?;
    assert_eq!(projects.len(), count);
    assert!(projects.iter().all(|project| {
        project
            .get("answer")
            .and_then(|answer| answer.get("kind"))
            .and_then(serde_json::Value::as_str)
            == Some(expected_answer)
    }));
    Ok(())
}

async fn assert_project_description(
    root: &Path,
    project: &str,
    description: &str,
    workspace: &str,
    board: &str,
) -> E2eResult {
    assert_eq!(
        project_state_rows(root, project).await?,
        vec![json!([
            project,
            description,
            workspace,
            board,
            1,
            1,
            null,
            "legacy_unknown"
        ])]
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn set_description_variants(source: &Path) -> E2eResult {
    let maximum = "λ\0\n".repeat(4096);
    assert_eq!(maximum.len(), message_board::MAX_DESCRIPTION_BYTES);
    let connection = rusqlite::Connection::open(source)?;
    for (topic, description) in [
        ("019a0000-0000-7000-8000-000000000015", String::new()),
        (
            "019a0000-0000-7000-8000-000000000016",
            "  first\r\nsecond\0\tline\n ".to_owned(),
        ),
        ("019a0000-0000-7000-8000-000000000017", maximum),
    ] {
        connection.execute(
            "UPDATE board_topics SET description=? WHERE topic_id=?",
            rusqlite::params![description, topic],
        )?;
    }
    connection.close().map_err(|(_, error)| error)?;
    Ok(())
}

fn change_topic_description(source: &Path) -> E2eResult {
    let connection = rusqlite::Connection::open(source)?;
    connection.execute(
        "UPDATE board_topics SET description=? WHERE topic_id=?",
        [
            "Changed semantic description",
            "019a0000-0000-7000-8000-000000000015",
        ],
    )?;
    connection.close().map_err(|(_, error)| error)?;
    Ok(())
}

fn insert_empty_topic(source: &Path) -> E2eResult {
    let connection = rusqlite::Connection::open(source)?;
    connection.execute(
        "INSERT INTO board_topics(topic_id,board_id,name,description) VALUES(?,?,?,?)",
        [
            "019a0000-0000-7000-8000-000000000019",
            "019a0000-0000-7000-8000-00000000000b",
            "ProjectState only",
            "A project without posts or activity",
        ],
    )?;
    connection.close().map_err(|(_, error)| error)?;
    Ok(())
}
