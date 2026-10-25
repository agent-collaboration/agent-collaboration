use crate::e2e_support::LegacySelections;
use crate::e2e_support::{
    CentralHarness, CliImportFixture, E2eResult, SourceSnapshot, feed_rows, mark_rows,
    membership_rows, post_rows, project_state_rows, project_watch_rows, read_legacy_selections,
    required_readback_cell, run_cli, thread_participation_rows, thread_watch_rows,
};
use crate::post_selection::{MainStartCutoff, ReaderCutoffs, ThreadCutoff, ThreadWatchCutoff};
use codex_router_test_support::scenario_finalization::run_scenario;
use serde_json::{Value, json};
use std::path::Path;

#[tokio::test]
async fn actual_cli_publishes_literal_history_and_selection_state() -> E2eResult {
    let fixture = CliImportFixture::new("legacy-complete.sqlite")?;
    let source = SourceSnapshot::capture(&fixture.source)?;
    let identity = std::fs::read(&fixture.identity_file)?;
    let selections = read_legacy_selections(
        &fixture.source,
        fixture.directory.path(),
        "human:legacy-owner",
        "019a0000-0000-7000-8000-000000000015",
    )?;
    assert_legacy_cutoffs(&selections)?;
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
                "legacy-owner",
                "legacy fixture machine",
            )
            .await?;
            assert_eq!(
                output.status.code(),
                Some(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let report = crate::e2e_support::read_report(&fixture.report)?;
            assert_uploaded_report(&report, 3, "imported")?;
            assert_eq!(
                project_report(&report, "019a0000-0000-7000-8000-000000000015")?
                    .get("feedRows")
                    .and_then(Value::as_u64),
                Some(24)
            );
            source.assert_unchanged(&fixture.source)?;
            assert_eq!(std::fs::read(&fixture.identity_file)?, identity);
            assert_router_lock_artifacts(&fixture.router_root)?;

            central.stop_owner().await?;
            assert_topic_15_state(central.root()).await?;
            assert_topic_15_feed(central.root()).await?;
            assert_topic_15_posts(central.root(), &selections.latest).await?;
            assert_eq!(
                selections.unread,
                [
                    "019a0000-0000-7000-8000-000000000068",
                    "019a0000-0000-7000-8000-0000000000cc",
                    "019a0000-0000-7000-8000-0000000000cd",
                ]
                .map(str::to_owned)
                .to_vec()
            );
            assert_topic_15_cutoffs(central.root()).await?;
            assert_topic_15_membership(central.root()).await?;
            assert_draining_membership(central.root()).await?;
            let projection = crate::upload_support::projection(central.root(), 3)
                .await
                .map_err(crate::e2e_support::promote_error)?;
            assert_eq!(projection.len(), 3);
            Ok(())
        },
        CentralHarness::shutdown,
        || Ok(()),
    )
    .await
}

fn assert_router_lock_artifacts(router_root: &Path) -> E2eResult {
    let mut entries = std::fs::read_dir(router_root)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    assert_eq!(
        entries,
        [
            std::ffi::OsString::from("agent-communication"),
            std::ffi::OsString::from("host.lock"),
        ]
    );
    Ok(())
}

fn assert_legacy_cutoffs(selections: &LegacySelections) -> E2eResult {
    assert_eq!(
        selections.cutoffs,
        ReaderCutoffs {
            reader: "human:legacy-owner".to_owned(),
            main_starts: vec![MainStartCutoff {
                legacy_workspace: "019a0000-0000-7000-8000-000000000001".to_owned(),
                main_start: Some(9),
            }],
            topic_mark_through_activity: 1,
            thread_cutoffs: vec![ThreadCutoff {
                root_id: Some("019a0000-0000-7000-8000-000000000065".to_owned()),
                watch: ThreadWatchCutoff::Present {
                    active: true,
                    starts_after_activity: 9,
                },
                thread_mark_through_activity: 11,
            }],
        }
    );
    Ok(())
}

fn assert_uploaded_report(report: &Value, count: usize, answer: &str) -> E2eResult {
    assert_eq!(report.get("kind").and_then(Value::as_str), Some("uploaded"));
    let projects = report
        .get("publication")
        .and_then(|publication| publication.get("projects"))
        .and_then(Value::as_array)
        .ok_or("publication projects missing")?;
    assert_eq!(projects.len(), count);
    assert!(projects.iter().all(|project| {
        project
            .get("answer")
            .and_then(|answer_result| answer_result.get("kind"))
            .and_then(Value::as_str)
            == Some(answer)
    }));
    Ok(())
}

fn project_report<'report>(report: &'report Value, project: &str) -> E2eResult<&'report Value> {
    report
        .get("report")
        .and_then(|report| report.get("projects"))
        .and_then(Value::as_array)
        .ok_or("project reports missing")?
        .iter()
        .find(|row| row.get("project").and_then(Value::as_str) == Some(project))
        .ok_or_else(|| crate::e2e_support::promote_error("project report missing"))
}

async fn assert_topic_15_state(root: &Path) -> E2eResult {
    let state = project_state_rows(root, "019a0000-0000-7000-8000-000000000015").await?;
    assert_eq!(
        state,
        vec![json!([
            "019a0000-0000-7000-8000-000000000015",
            "Project description 21",
            "019a0000-0000-7000-8000-000000000001",
            "019a0000-0000-7000-8000-00000000000b",
            1,
            1,
            null,
            "legacy_unknown"
        ])]
    );
    Ok(())
}

async fn assert_topic_15_feed(root: &Path) -> E2eResult {
    let feed = feed_rows(root, "019a0000-0000-7000-8000-000000000015").await?;
    assert_eq!(feed.len(), 24);
    for (offset, row) in feed.iter().enumerate() {
        assert_eq!(required_readback_cell(row, 0)?, &json!(1));
        assert_eq!(required_readback_cell(row, 1)?, &json!(offset as u64 + 1));
        assert_eq!(required_readback_cell(row, 4)?, &Value::Null);
        assert_eq!(
            required_readback_cell(row, 5)?.as_str(),
            Some("legacy_unknown")
        );
    }
    assert!(
        feed.iter()
            .skip(18)
            .all(|row| row.get(2).and_then(Value::as_str) == Some("subscriptionMembership"))
    );
    let first = feed.first().ok_or("first feed row missing")?;
    assert_eq!(required_readback_cell(first, 2)?.as_str(), Some("project"));
    assert_eq!(
        feed.get(1).ok_or("second feed row missing")?,
        &json!([
            1,
            2,
            "thread",
            "019a0000-0000-7000-8000-000000000065",
            null,
            "legacy_unknown"
        ])
    );
    assert_eq!(
        feed.get(2).ok_or("third feed row missing")?,
        &json!([
            1,
            3,
            "post",
            "019a0000-0000-7000-8000-000000000065",
            null,
            "legacy_unknown"
        ])
    );
    Ok(())
}

async fn assert_topic_15_posts(root: &Path, oracle_latest: &[String]) -> E2eResult {
    let posts = post_rows(root, "019a0000-0000-7000-8000-000000000015").await?;
    assert_eq!(posts, expected_topic_15_posts());
    let actual_latest = posts
        .iter()
        .rev()
        .map(|row| {
            required_readback_cell(row, 0)?
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| crate::e2e_support::promote_error("post id missing"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let expected_latest = [
        "019a0000-0000-7000-8000-0000000000ce",
        "019a0000-0000-7000-8000-0000000000cd",
        "019a0000-0000-7000-8000-0000000000cc",
        "019a0000-0000-7000-8000-000000000068",
        "019a0000-0000-7000-8000-0000000000cb",
        "019a0000-0000-7000-8000-0000000000c9",
        "019a0000-0000-7000-8000-000000000065",
    ]
    .map(str::to_owned);
    assert_eq!(oracle_latest, expected_latest.to_vec());
    assert_eq!(actual_latest, oracle_latest);
    Ok(())
}

fn expected_topic_15_posts() -> Vec<Value> {
    vec![
        json!([
            "019a0000-0000-7000-8000-000000000065",
            "019a0000-0000-7000-8000-000000000065",
            "project",
            expected_session_actor("creator-a"),
            null,
            "orchestrator",
            3
        ]),
        json!([
            "019a0000-0000-7000-8000-0000000000c9",
            "019a0000-0000-7000-8000-000000000065",
            "thread",
            expected_session_actor("reviewer"),
            expected_human_actor("legacy-owner"),
            "reviewer",
            5
        ]),
        json!([
            "019a0000-0000-7000-8000-0000000000cb",
            "019a0000-0000-7000-8000-000000000065",
            "thread",
            expected_session_actor("reviewer"),
            null,
            "orchestrator",
            7
        ]),
        json!([
            "019a0000-0000-7000-8000-000000000068",
            "019a0000-0000-7000-8000-000000000068",
            "project",
            expected_session_actor("creator-d"),
            null,
            "orchestrator",
            9
        ]),
        json!([
            "019a0000-0000-7000-8000-0000000000cc",
            "019a0000-0000-7000-8000-000000000065",
            "thread",
            expected_session_actor("reviewer"),
            null,
            "advisor",
            12
        ]),
        json!([
            "019a0000-0000-7000-8000-0000000000cd",
            "019a0000-0000-7000-8000-000000000065",
            "thread",
            expected_session_actor("reviewer"),
            null,
            "implementer",
            15
        ]),
        json!([
            "019a0000-0000-7000-8000-0000000000ce",
            "019a0000-0000-7000-8000-000000000065",
            "thread",
            expected_human_actor("legacy-owner"),
            null,
            null,
            18
        ]),
    ]
}

fn expected_session_actor(session_id: &str) -> Value {
    json!({
        "kind": "session",
        "session": {
            "endpoint": {
                "serviceId": "550e8400-e29b-41d4-a716-446655440000",
                "endpointId": "codex-local"
            },
            "sessionId": session_id
        }
    })
}

fn expected_human_actor(human_id: &str) -> Value {
    json!({"kind":"human","humanId":human_id})
}

async fn assert_topic_15_cutoffs(root: &Path) -> E2eResult {
    let project = "019a0000-0000-7000-8000-000000000015";
    let reader = expected_human_actor("legacy-owner");
    let marks = mark_rows(root, project).await?;
    let selected = marks
        .iter()
        .filter(|row| row.get(0) == Some(&reader))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        selected,
        vec![
            json!([reader.clone(), "project", project, 1, 5]),
            json!([
                reader.clone(),
                "thread",
                "019a0000-0000-7000-8000-000000000065",
                1,
                7
            ]),
        ]
    );
    let thread_watches = thread_watch_rows(root, project).await?;
    assert!(thread_watches.contains(&json!([
        reader.clone(),
        "019a0000-0000-7000-8000-000000000065",
        1,
        5,
        true
    ])));
    assert!(thread_watches.contains(&json!([
        expected_session_actor("reviewer"),
        "019a0000-0000-7000-8000-000000000065",
        1,
        4,
        false
    ])));
    assert!(thread_watches.contains(&json!([
        expected_session_actor("creator-a"),
        "019a0000-0000-7000-8000-000000000065",
        1,
        10,
        true
    ])));
    let project_watches = project_watch_rows(root, project).await?;
    assert!(project_watches.contains(&json!([reader, 1, 5, true])));
    let participants = thread_participation_rows(root, project).await?;
    assert_participant_intervals(&participants)?;
    Ok(())
}

fn assert_participant_intervals(participants: &[Value]) -> E2eResult {
    let reviewer = expected_session_actor("reviewer");
    let reviewer_intervals = participants
        .iter()
        .filter(|row| row.get(0) == Some(&reviewer))
        .map(|row| {
            Ok(json!([
                required_readback_cell(row, 1)?,
                required_readback_cell(row, 3)?,
                required_readback_cell(row, 7)?,
                required_readback_cell(row, 8)?,
                required_readback_cell(row, 9)?
            ]))
        })
        .collect::<E2eResult<Vec<_>>>()?;
    assert_eq!(
        reviewer_intervals,
        vec![
            json!(["reviewer", 4, 6, "promoted", null]),
            json!([
                "orchestrator",
                6,
                10,
                "replaced",
                expected_session_actor("creator-a")
            ]),
            json!(["advisor", 11, 14, "rejoined", null]),
            json!(["implementer", 14, 16, "left", null]),
        ]
    );
    let implementer = expected_session_actor("implementer");
    assert!(participants.iter().any(|row| {
        row.get(0) == Some(&implementer)
            && row.get(1).and_then(Value::as_str) == Some("implementer")
            && row.get(3).and_then(Value::as_u64) == Some(13)
            && row.get(7).and_then(Value::as_u64) == Some(14)
            && row.get(8).and_then(Value::as_str) == Some("replaced")
            && row.get(9) == Some(&reviewer)
    }));
    Ok(())
}

async fn assert_topic_15_membership(root: &Path) -> E2eResult {
    let rows = membership_rows(root, "019a0000-0000-7000-8000-000000000015").await?;
    let owner = expected_human_actor("legacy-owner");
    let membership = rows
        .iter()
        .find(|row| {
            row.get(0) == Some(&owner)
                && row.get(1).and_then(Value::as_str) == Some("project")
                && row.get(2).and_then(Value::as_str)
                    == Some("019a0000-0000-7000-8000-000000000015")
        })
        .ok_or("human project membership absent")?;
    let policy: Value = serde_json::from_str(
        required_readback_cell(membership, 3)?
            .as_str()
            .ok_or("policy absent")?,
    )?;
    assert_eq!(
        policy,
        json!({"mode":"poll","whenIdle":"drop","timing":{"quietSeconds":4,"capSeconds":12},"lifetime":1200})
    );
    assert_timestamp(
        required_readback_cell(membership, 4)?,
        "2023-11-14T22:21:50Z",
    )?;
    assert_timestamp(
        required_readback_cell(membership, 5)?,
        "2023-11-14T22:41:50Z",
    )?;
    assert_eq!(required_readback_cell(membership, 6)?.as_u64(), Some(2));
    let ended: Value = serde_json::from_str(
        required_readback_cell(membership, 9)?
            .as_str()
            .ok_or("end record absent")?,
    )?;
    assert_eq!(
        ended
            .get("reason")
            .and_then(|reason| reason.get("kind"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    Ok(())
}

fn assert_timestamp(value: &Value, expected: &str) -> E2eResult {
    let actual = chrono::DateTime::parse_from_rfc3339(
        value.as_str().ok_or("membership timestamp missing")?,
    )?
    .with_timezone(&chrono::Utc);
    let expected = chrono::DateTime::parse_from_rfc3339(expected)?.with_timezone(&chrono::Utc);
    assert_eq!(actual, expected);
    Ok(())
}

async fn assert_draining_membership(root: &Path) -> E2eResult {
    let rows = membership_rows(root, "019a0000-0000-7000-8000-000000000017").await?;
    let creator = expected_session_actor("creator-b");
    let membership = rows
        .iter()
        .find(|row| {
            row.get(0) == Some(&creator)
                && row.get(1).and_then(Value::as_str) == Some("thread")
                && row.get(2).and_then(Value::as_str)
                    == Some("019a0000-0000-7000-8000-000000000066")
        })
        .ok_or_else(|| {
            crate::e2e_support::promote_error(format!(
                "draining thread membership absent from native rows: {rows:?}"
            ))
        })?;
    let policy: Value = serde_json::from_str(
        required_readback_cell(membership, 3)?
            .as_str()
            .ok_or("policy absent")?,
    )?;
    assert_eq!(
        policy,
        json!({"mode":"deliver","whenIdle":"wake","timing":{"quietSeconds":3,"capSeconds":9},"lifetime":3600})
    );
    assert_timestamp(
        required_readback_cell(membership, 5)?,
        "2023-11-14T23:21:40Z",
    )?;
    assert_eq!(required_readback_cell(membership, 6)?.as_u64(), Some(3));
    let ended: Value = serde_json::from_str(
        required_readback_cell(membership, 9)?
            .as_str()
            .ok_or("end record absent")?,
    )?;
    assert_eq!(
        ended
            .get("reason")
            .and_then(|reason| reason.get("kind"))
            .and_then(Value::as_str),
        Some("resolved")
    );
    Ok(())
}
