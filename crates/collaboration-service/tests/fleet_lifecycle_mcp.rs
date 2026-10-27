#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Workspace archival and initiative closure travel through fleet admission.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::json;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn workspace_archive_cascades_atomically_and_retains_pending_project_reports()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    fixture.project().await?;
    let empty_board = BoardId::generate();
    accepted(&keyed_call(&fixture,"board_create",json!({"workspaceId":fixture.workspace,"boardId":empty_board,"name":"empty board","description":"","kind":{"kind":"area"}})).await?);
    let (other_workspace, other_board, _) = create_location(&fixture, "untouched").await?;
    let mut request = fixture.header();
    request
        .as_object_mut()
        .ok_or("header")?
        .insert("workspaceId".to_owned(), json!(fixture.workspace));
    let reply = fixture
        .api
        .call("workspace_archive", request.clone())
        .await?;
    accepted(&reply);
    assert_eq!(
        reply
            .pointer("/result/result/decision/effects/entries")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(
        fixture
            .api
            .call("workspace_show", json!({"workspaceId":fixture.workspace}))
            .await?
            .pointer("/result/workspace/state"),
        Some(&json!("archiving"))
    );
    assert_eq!(
        fixture
            .api
            .call("board_show", json!({"boardId":fixture.board}))
            .await?
            .pointer("/result/board/state"),
        Some(&json!("archiving"))
    );
    assert_eq!(
        fixture
            .api
            .call("board_show", json!({"boardId":empty_board}))
            .await?
            .pointer("/result/board/state"),
        Some(&json!("archived"))
    );
    assert_eq!(
        fixture
            .api
            .call("workspace_show", json!({"workspaceId":other_workspace}))
            .await?
            .pointer("/result/workspace/state"),
        Some(&json!("active"))
    );
    assert_eq!(
        fixture
            .api
            .call("board_show", json!({"boardId":other_board}))
            .await?
            .pointer("/result/board/state"),
        Some(&json!("open"))
    );
    let replay = fixture
        .api
        .call("workspace_archive", request.clone())
        .await?;
    assert_eq!(replay.get("result"), reply.get("result"));
    let fixture = fixture.restart_read_fixture().await?;
    assert_eq!(
        fixture
            .api
            .call("workspace_archive", request)
            .await?
            .get("result"),
        reply.get("result")
    );
    let empty_workspace = WorkspaceId::generate();
    accepted(
        &keyed_call(
            &fixture,
            "workspace_create",
            json!({"workspaceId":empty_workspace,"name":"no boards","description":""}),
        )
        .await?,
    );
    accepted(
        &keyed_call(
            &fixture,
            "workspace_archive",
            json!({"workspaceId":empty_workspace}),
        )
        .await?,
    );
    assert_eq!(
        fixture
            .api
            .call("workspace_show", json!({"workspaceId":empty_workspace}))
            .await?
            .pointer("/result/workspace/state"),
        Some(&json!("archived"))
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn board_close_accepts_only_initiatives_and_keeps_replay_and_noop_meaning()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let board = BoardId::generate();
    accepted(&keyed_call(&fixture,"board_create",json!({"workspaceId":fixture.workspace,"boardId":board,"name":"initiative","description":"","kind":{"kind":"initiative","goal":"ship","doneWhen":"accepted"}})).await?);
    let mut request = fixture.header();
    request
        .as_object_mut()
        .ok_or("header")?
        .insert("boardId".to_owned(), json!(board));
    let reply = fixture.api.call("board_close", request.clone()).await?;
    accepted(&reply);
    assert_eq!(
        reply.pointer("/result/result/decision/change"),
        Some(&json!("changed"))
    );
    assert_eq!(
        fixture
            .api
            .call("board_show", json!({"boardId":board}))
            .await?
            .pointer("/result/board/state"),
        Some(&json!("closed"))
    );
    assert_eq!(
        fixture
            .api
            .call("board_close", request)
            .await?
            .get("result"),
        reply.get("result")
    );
    let noop = keyed_call(&fixture, "board_close", json!({"boardId":board})).await?;
    accepted(&noop);
    assert_eq!(
        noop.pointer("/result/result/decision/change"),
        Some(&json!("alreadySo"))
    );
    let refusal = keyed_call(&fixture, "board_close", json!({"boardId":fixture.board})).await?;
    assert_eq!(
        refusal.pointer("/result/result/decision/reasons/0/kind"),
        Some(&json!("invalidShape")),
        "{refusal}"
    );
    assert_eq!(
        fixture
            .api
            .call("board_show", json!({"boardId":fixture.board}))
            .await?
            .pointer("/result/board/state"),
        Some(&json!("open"))
    );
    fixture.stop().await
}

async fn supplied_report(
    fixture: &BoardFixture,
    board: &BoardId,
    project: &ProjectId,
) -> Result<fleet_admission::ArchiveReportOutcome, TestError> {
    let report = fleet_admission::ArchiveReport {
        board: board.clone(),
        project: project.clone(),
        position: serde_json::from_value(json!({"epoch":1,"sequence":7}))?,
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        reported_at: chrono::Utc::now(),
    };
    let outcome = fixture._fleet.report_archive_seen(report).await?;
    // The report writes central; public metadata reads use the Router's cache.
    fixture._fleet.synchronize().await?;
    Ok(outcome)
}

async fn fleet_snapshot(
    fixture: &BoardFixture,
) -> Result<message_board_storage::FleetRecordSnapshot, TestError> {
    let (records, through) = fixture._fleet_runtime.fleet_snapshot().await?;
    Ok(message_board_storage::FleetRecordSnapshot { records, through })
}

async fn fleet_ledger(fixture: &BoardFixture) -> Result<Vec<Record>, TestError> {
    let records = fleet_snapshot(fixture)
        .await?
        .records
        .into_iter()
        .filter(|record| matches!(record, Record::Request(_)))
        .collect();
    Ok(records)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn supplied_reports_finish_one_and_multiple_boards_in_the_same_fleet_decision()
-> Result<(), TestError> {
    for multiple_boards in [false, true] {
        let fixture = BoardFixture::start().await?;
        let (first, _) = fixture.project().await?;
        let (second, _) = fixture.project().await?;
        let extra_board = BoardId::generate();
        let extra_project = ProjectId::generate();
        if multiple_boards {
            accepted(&keyed_call(&fixture,"board_create",json!({"workspaceId":fixture.workspace,"boardId":extra_board,"name":"second pending board","description":"","kind":{"kind":"area"}})).await?);
            accepted(&create_registered_project(&fixture,json!({"boardId":extra_board,"projectId":extra_project,"title":"other pending project"})).await?);
        }
        accepted(
            &keyed_call(
                &fixture,
                "workspace_archive",
                json!({"workspaceId":fixture.workspace}),
            )
            .await?,
        );
        let ledger_before_reports = fleet_ledger(&fixture).await?;
        let invalid = supplied_report(&fixture, &fixture.board, &ProjectId::generate()).await?;
        assert!(
            matches!(
                invalid,
                fleet_admission::ArchiveReportOutcome::Refused(
                    fleet_admission::ArchiveReportRefusal::NotFound(_)
                )
            ),
            "{invalid:?}"
        );
        assert_eq!(
            fleet_ledger(&fixture).await?,
            ledger_before_reports,
            "an automated refusal must write no request-ledger entry"
        );
        let refusal=keyed_call(&fixture,"workspace_update",json!({"workspaceId":fixture.workspace,"name":"should stay untouched","description":""})).await?;
        assert_eq!(
            refusal.pointer("/result/result/decision/reasons/0/kind"),
            Some(&json!("archived"))
        );
        let ledger_before_reports = fleet_ledger(&fixture).await?;
        let before_first = fleet_snapshot(&fixture).await?;
        assert_eq!(
            supplied_report(&fixture, &fixture.board, &first).await?,
            fleet_admission::ArchiveReportOutcome::Recorded
        );
        assert_eq!(
            fleet_snapshot(&fixture).await?,
            before_first,
            "a partial report changes no fleet record/feed"
        );
        assert_eq!(
            supplied_report(&fixture, &fixture.board, &first).await?,
            fleet_admission::ArchiveReportOutcome::AlreadyRecorded
        );
        assert_eq!(
            fleet_snapshot(&fixture).await?,
            before_first,
            "repeat report is a no-op"
        );
        assert_eq!(
            fixture
                .api
                .call("board_show", json!({"boardId":fixture.board}))
                .await?
                .pointer("/result/board/state"),
            Some(&json!("archiving"))
        );
        assert_eq!(
            fixture
                .api
                .call("workspace_show", json!({"workspaceId":fixture.workspace}))
                .await?
                .pointer("/result/workspace/state"),
            Some(&json!("archiving"))
        );
        let mut before_final = fleet_snapshot(&fixture).await?;
        let mut final_board = fixture.board.clone();
        let mut final_started = chrono::Utc::now();
        assert_eq!(
            supplied_report(&fixture, &fixture.board, &second).await?,
            fleet_admission::ArchiveReportOutcome::Recorded
        );
        if multiple_boards {
            assert_eq!(
                fixture
                    .api
                    .call("board_show", json!({"boardId":fixture.board}))
                    .await?
                    .pointer("/result/board/state"),
                Some(&json!("archived"))
            );
            assert_eq!(
                fixture
                    .api
                    .call("workspace_show", json!({"workspaceId":fixture.workspace}))
                    .await?
                    .pointer("/result/workspace/state"),
                Some(&json!("archiving"))
            );
            before_final = fleet_snapshot(&fixture).await?;
            final_board = extra_board.clone();
            final_started = chrono::Utc::now();
            // Automated identity is (board, project), with no conflicting request key.
            assert_eq!(
                supplied_report(&fixture, &extra_board, &extra_project).await?,
                fleet_admission::ArchiveReportOutcome::Recorded
            );
        }
        let final_finished = chrono::Utc::now();
        let board_sync_protocol::FleetFeedRead::Page {
            records,
            through: feed_through,
            head,
            ..
        } = fixture
            ._fleet_runtime
            .fleet_feed(before_final.through)
            .await?
        else {
            return Err("finalization did not return its authoritative feed page".into());
        };
        let entries = records
            .into_iter()
            .map(|payload| -> Result<_, TestError> {
                let record: Record = payload.decode()?;
                let position = record
                    .envelope()
                    .position
                    .fleet_position()
                    .ok_or("feed record has no Fleet position")?;
                Ok(fleet_admission::FleetFeedEntry { position, record })
            })
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(
            feed_through, head,
            "the finalization feed is fully observed"
        );
        assert_eq!(
            entries.len(),
            2,
            "board and workspace finish in the same transaction without a ledger event"
        );
        let mut expected = Vec::new();
        let mut through = before_final.through;
        for reference in [
            RecordRef::from(final_board),
            RecordRef::from(fixture.workspace.clone()),
        ] {
            let mut record = before_final
                .records
                .iter()
                .find(|record| record.reference() == reference)
                .ok_or("archiving record")?
                .clone();
            match &mut record {
                Record::Board(board) => board.state = BoardState::Archived,
                Record::Workspace(workspace) => workspace.state = WorkspaceState::Archived,
                _ => return Err("wrong archive record".into()),
            }
            through = through.successor()?;
            let observed_time = entries
                .first()
                .ok_or("finalizing feed")?
                .record
                .envelope()
                .time
                .as_router_time()
                .ok_or("finalizing router time")?;
            // Central's SyncTime clock is milliseconds; compare at that precision.
            assert!(
                observed_time.timestamp_millis() >= final_started.timestamp_millis()
                    && observed_time.timestamp_millis() <= final_finished.timestamp_millis(),
                "finalization timestamp {observed_time} is outside {final_started}..={final_finished}"
            );
            let envelope = record.envelope_mut();
            envelope.position = RecordPlacement::Fleet(FleetFeedEntry {
                position: through,
                origin: envelope.position.origin().cloned().ok_or("fleet origin")?,
            });
            envelope.request = None;
            envelope.time = RecordTime::Router(observed_time);
            expected.push(fleet_admission::FleetFeedEntry {
                position: through,
                record,
            });
        }
        assert_eq!(
            entries, expected,
            "full records retain the original archive actors, identity and scope"
        );
        assert_eq!(fleet_ledger(&fixture).await?, ledger_before_reports);
        assert_eq!(
            fixture
                .api
                .call("workspace_show", json!({"workspaceId":fixture.workspace}))
                .await?
                .pointer("/result/workspace/state"),
            Some(&json!("archived"))
        );
        let finished = fleet_snapshot(&fixture).await?;
        assert_eq!(
            supplied_report(&fixture, &fixture.board, &second).await?,
            fleet_admission::ArchiveReportOutcome::AlreadyRecorded
        );
        assert_eq!(fleet_snapshot(&fixture).await?, finished);
        assert_eq!(fleet_ledger(&fixture).await?, ledger_before_reports);
        fixture.stop().await?;
    }
    Ok(())
}
