use crate::*;
use message_board::*;
type TestResult<TValue = ()> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn archive_report_preserves_the_native_position_without_a_legacy_commit_counter() -> TestResult {
    let (report, _) = fixture()?;
    let mut value = serde_json::to_value(report)?;
    let position = serde_json::json!({"epoch":2,"sequence":9_007_199_254_740_993_u64});
    value["position"] = position.clone();
    let decoded: ArchiveReport = serde_json::from_value(value)?;
    assert_eq!(serde_json::to_value(decoded)?["position"], position);
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn archive_report_rejects_the_legacy_position_shape() -> TestResult {
    let (report, _) = fixture()?;
    let mut value = serde_json::to_value(report)?;
    value["position"] = serde_json::json!({"epoch":2,"commitId":7});
    assert!(serde_json::from_value::<ArchiveReport>(value).is_err());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn archive_reports_have_no_legacy_request_or_write_encoding() -> TestResult {
    let (report, _) = fixture()?;
    assert_eq!(
        serde_json::from_value::<ArchiveReport>(serde_json::to_value(&report)?)?,
        report
    );
    assert!(serde_json::from_value::<FleetOperation>(serde_json::json!({"kind":"archiveSeen", "board":report.board, "project":report.project})).is_err());
    assert!(serde_json::from_value::<FleetWrite>(serde_json::json!({"kind":"archiveReport", "record":{"board":report.board, "project":report.project}})).is_err());
    Ok(())
}

fn fixture() -> TestResult<(ArchiveReport, FleetSnapshot)> {
    let source = |number| format!("019f0000-0000-7000-8000-{number:012x}");
    let workspace = WorkspaceId::try_from(source(1))?;
    let board = BoardId::try_from(source(2))?;
    let project = ProjectId::try_from(source(3))?;
    let other = ProjectId::try_from(source(4))?;
    let provenance = ImportProvenance {
        installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
        machine: MachineLabel::try_from("archive requester".to_owned())?,
    };
    let time = RecordTime::Router("2020-01-02T03:04:05Z".parse()?);
    let bundle = FleetImportBundle {
        declared_owner: HumanId::try_from("owner".to_owned())?,
        provenance: provenance.clone(),
        records: vec![
            FleetImportRecord::Workspace {
                id: workspace.clone(),
                name: ResourceName::try_from("archive workspace".to_owned())?,
                description: Description::try_from("archive description".to_owned())?,
                repositories: vec![],
                time,
            },
            FleetImportRecord::Board {
                id: board.clone(),
                workspace,
                name: ResourceName::try_from("archive board".to_owned())?,
                description: Description::try_from("archive description".to_owned())?,
                state: ImportedBoardState::Active,
                time,
            },
            FleetImportRecord::Directory {
                project: project.clone(),
                board: board.clone(),
                title: ResourceName::try_from("first project".to_owned())?,
                time,
            },
            FleetImportRecord::Directory {
                project: other,
                board: board.clone(),
                title: ResourceName::try_from("other project".to_owned())?,
                time,
            },
        ],
    };
    let imported = decide_import(&bundle, &FleetSnapshot::empty())?;
    let mut snapshot = FleetSnapshot::empty();
    for write in imported.writes {
        let mut record = write.record;
        match &mut record {
            Record::Workspace(value) => value.state = WorkspaceState::Archiving,
            Record::Board(value) => {
                value.state = BoardState::Archiving;
                snapshot.workspace_boards.push(value.clone());
            }
            Record::DirectoryEntry(value) => snapshot.board_projects.push(value.clone()),
            _ => {}
        }
        snapshot.records.push(record);
    }
    let report = ArchiveReport {
        board,
        project,
        position: fleet_sync_protocol::ReplicationPosition::new(
            fleet_sync_protocol::Epoch::new(1)?,
            fleet_sync_protocol::CommitSequence::new(2),
        ),
        installation: provenance.installation,
        reported_at: "2026-10-16T00:00:00Z".parse()?,
    };
    Ok((report, snapshot))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn first_partial_report_records_once_and_successor_resend_is_noop() -> TestResult {
    let (mut report, mut snapshot) = fixture()?;
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: true,
            finalize: None
        }
    );
    snapshot.archive_reports.push(report.project.clone());
    report.installation = ServiceId::try_from("00000000-0000-4000-8000-000000000002".to_owned())?;
    report.position.sequence = fleet_sync_protocol::CommitSequence::new(999);
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: false,
            finalize: None
        }
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn final_report_finishes_last_board_and_workspace_but_not_other_open_board() -> TestResult {
    let (report, mut snapshot) = fixture()?;
    snapshot.archive_reports.extend(
        snapshot
            .board_projects
            .iter()
            .filter(|entry| entry.id != report.project)
            .map(|entry| entry.id.clone()),
    );
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: true,
            finalize: Some(ArchiveFinalization {
                board: report.board.clone(),
                workspace: true
            })
        }
    );
    let mut other = snapshot.workspace_boards[0].clone();
    other.board_id = BoardId::try_from("019f0000-0000-7000-8000-000000000099".to_owned())?;
    snapshot.workspace_boards.push(other);
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: true,
            finalize: Some(ArchiveFinalization {
                board: report.board,
                workspace: false
            })
        }
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn unheld_project_needs_no_report_and_completed_repeat_has_no_finalization() -> TestResult {
    let (report, mut snapshot) = fixture()?;
    snapshot.unheld_projects.extend(
        snapshot
            .board_projects
            .iter()
            .filter(|entry| entry.id != report.project)
            .map(|entry| entry.id.clone()),
    );
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: true,
            finalize: Some(ArchiveFinalization {
                board: report.board.clone(),
                workspace: true
            })
        }
    );
    snapshot.archive_reports.push(report.project.clone());
    for record in &mut snapshot.records {
        match record {
            Record::Board(value) => value.state = BoardState::Archived,
            Record::Workspace(value) => value.state = WorkspaceState::Archived,
            _ => {}
        }
    }
    assert_eq!(
        decide_archive_report(&report, &snapshot)
            .map_err(|refusal| format!("unexpected archive refusal: {refusal:?}"))?,
        ArchiveReportDecision {
            record: false,
            finalize: None
        }
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn archive_parent_exemption_keeps_board_project_and_state_checks() -> TestResult {
    let (mut report, mut snapshot) = fixture()?;
    let missing = ProjectId::try_from("019f0000-0000-7000-8000-000000000099".to_owned())?;
    report.project = missing.clone();
    assert_eq!(
        decide_archive_report(&report, &snapshot),
        Err(ArchiveReportRefusal::NotFound(missing.into()))
    );
    report.project = snapshot.board_projects[0].id.clone();
    for record in &mut snapshot.records {
        if let Record::Board(value) = record {
            value.state = BoardState::Open;
        }
    }
    assert_eq!(
        decide_archive_report(&report, &snapshot),
        Err(ArchiveReportRefusal::BoardNotArchiving {
            state: BoardState::Open
        })
    );
    Ok(())
}
