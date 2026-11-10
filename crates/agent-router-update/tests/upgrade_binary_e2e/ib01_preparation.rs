use crate::e2e_support::{CliImportFixture, E2eResult, SourceSnapshot, read_report, run_cli};
use message_board::MAX_DESCRIPTION_BYTES;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn actual_cli_dry_run_reports_literal_history_and_keeps_inputs_unchanged() -> E2eResult {
    let fixture = CliImportFixture::new("legacy-complete.sqlite")?;
    let descriptions = set_description_variants(&fixture.source)?;
    let source = SourceSnapshot::capture(&fixture.source)?;
    let identity = std::fs::read(&fixture.identity_file)?;
    let output = run_cli(
        &fixture.source,
        &fixture.router_root,
        &fixture.report,
        None,
        "dry-run-owner",
        "dry-run-machine",
    )
    .await?;

    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = read_report(&fixture.report)?;
    assert_eq!(report.get("kind").and_then(Value::as_str), Some("dryRun"));
    let sources = report.get("sources").ok_or("source report missing")?;
    assert_eq!(
        sources.get("owner").and_then(Value::as_str),
        Some("explicitOverride")
    );
    assert_eq!(
        sources.get("machineLabel").and_then(Value::as_str),
        Some("explicitOverride")
    );
    assert_dry_run_projects(&report, &descriptions)?;
    assert_project_15_mapping(&report)?;
    let fleet_records = report
        .get("report")
        .and_then(|report| report.get("fleet"))
        .and_then(|fleet| fleet.get("records"))
        .and_then(Value::as_array)
        .ok_or("fleet records missing")?;
    assert!(fleet_records.iter().all(|record| {
        record
            .get("time")
            .and_then(|time| time.get("kind"))
            .and_then(Value::as_str)
            == Some("legacyUnknown")
    }));
    source.assert_unchanged(&fixture.source)?;
    assert_eq!(std::fs::read(&fixture.identity_file)?, identity);
    assert!(
        !fixture
            .router_root
            .join("agent-communication/interaction.sqlite")
            .exists()
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn set_description_variants(path: &std::path::Path) -> E2eResult<Vec<String>> {
    let empty = String::new();
    let control = "  first\r\nsecond\0\tline\n ".to_owned();
    let maximum = "λ\0\n".repeat(4096);
    assert_eq!(maximum.len(), MAX_DESCRIPTION_BYTES);
    let connection = rusqlite::Connection::open(path)?;
    for (topic, description) in [
        ("019a0000-0000-7000-8000-000000000015", &empty),
        ("019a0000-0000-7000-8000-000000000016", &control),
        ("019a0000-0000-7000-8000-000000000017", &maximum),
    ] {
        connection.execute(
            "UPDATE board_topics SET description=? WHERE topic_id=?",
            rusqlite::params![description, topic],
        )?;
    }
    connection.close().map_err(|(_, error)| error)?;
    Ok(vec![empty, control, maximum])
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_dry_run_projects(report: &Value, descriptions: &[String]) -> E2eResult {
    let projects = report
        .get("report")
        .and_then(|report| report.get("projects"))
        .and_then(Value::as_array)
        .ok_or("project reports missing")?;
    assert_eq!(projects.len(), 3);
    assert_eq!(descriptions.len(), projects.len());
    let expected_titles = ["Project 21", "Project 22", "Project 23"];
    for (offset, ((project, description), title)) in projects
        .iter()
        .zip(descriptions)
        .zip(expected_titles)
        .enumerate()
    {
        let expected_id = format!("019a0000-0000-7000-8000-0000000000{}", 15 + offset);
        assert_eq!(
            project.get("project").and_then(Value::as_str),
            Some(expected_id.as_str())
        );
        assert_eq!(
            project.get("description").and_then(Value::as_str),
            Some(description.as_str())
        );
        assert_eq!(project.get("title").and_then(Value::as_str), Some(title));
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_project_15_mapping(report: &Value) -> E2eResult {
    let projects = report
        .get("report")
        .and_then(|report| report.get("projects"))
        .and_then(Value::as_array)
        .ok_or("projects missing")?;
    let project = projects
        .iter()
        .find(|row| {
            row.get("project").and_then(Value::as_str)
                == Some("019a0000-0000-7000-8000-000000000015")
        })
        .ok_or("Project 21 report missing")?;
    let mapping = project
        .get("oldToNew")
        .and_then(Value::as_object)
        .ok_or("old activity map missing")?;
    let mut actual = mapping
        .iter()
        .map(|(old, cursor)| {
            Ok((
                old.parse::<u64>()?,
                cursor
                    .get("epoch")
                    .and_then(Value::as_u64)
                    .ok_or("epoch missing")?,
                cursor
                    .get("position")
                    .and_then(Value::as_u64)
                    .ok_or("position missing")?,
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error + Send + Sync>>>()?;
    actual.sort_by_key(|(old, _, _)| *old);
    let expected_positions: [u64; 17] =
        [3, 3, 4, 5, 6, 7, 9, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
    let old_sequences: [u64; 17] = [
        1, 2, 7, 8, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
    ];
    let expected = old_sequences
        .into_iter()
        .zip(expected_positions)
        .map(|(old, position)| (old, 1_u64, position))
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(mapping.get("1"), Some(&json!({"epoch": 1, "position": 3})));
    Ok(())
}
