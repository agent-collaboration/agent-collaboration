#![allow(clippy::expect_used, clippy::panic)]
mod archive_support;
mod registry_support;
use archive_support::*;
use collaboration_central_core::*;
use collaboration_replication_protocol::{
    BoardId as WireBoardId, ProjectId as WireProjectId, SessionAnswer,
};
use fleet_admission::*;
use message_board::*;
use registry_support::{at, identity, machine, position};

fn outcome(answer: SessionAnswer) -> ArchiveReportOutcome {
    let SessionAnswer::ArchiveReport(value) = answer else {
        panic!("automated report acknowledgement");
    };
    serde_json::from_value(value.as_value().clone()).expect("typed outcome")
}

fn report(
    fixture: &mut ArchiveFixture,
    board: &BoardId,
    project: &ProjectId,
) -> ArchiveReportOutcome {
    let FleetResult::ArchiveSeen(answer) = fixture.call(
        FleetCall::ArchiveSeen {
            installation: machine(1),
            board: WireBoardId::new(board.as_str()).expect("board"),
            project: WireProjectId::new(project.as_str()).expect("project"),
            position: position(2, 7),
        },
        2,
    ) else {
        panic!("report result");
    };
    outcome(answer)
}

fn reports(fixture: &ArchiveFixture) -> Vec<(String, String, String, String, String, String)> {
    fixture.store.connection.prepare("SELECT board,project,position_epoch,position_commit,installation,reported_at FROM archive_reports ORDER BY board,project").expect("reports").query_map([], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).expect("rows").collect::<rusqlite::Result<_>>().expect("reports")
}

#[test]
fn first_report_retains_all_evidence_across_successor_resends_and_completion() {
    let mut fixture = ArchiveFixture::new();
    fixture.archive_workspace();
    let ledger = fixture.ledger();
    let before = fixture.feed();
    assert_eq!(
        outcome(fixture.report(0, 1, 2)),
        ArchiveReportOutcome::Recorded
    );
    let expected = vec![(
        fixture.board.as_str().to_owned(),
        fixture.projects.first().expect("first").as_str().to_owned(),
        "2".into(),
        "7".into(),
        machine(1).as_str().to_owned(),
        at(2).0.to_rfc3339(),
    )];
    assert_eq!(reports(&fixture), expected);
    let first_project = fixture.projects.first().expect("first").clone();
    let FleetResult::ArchiveSeen(answer) = fixture.call(
        FleetCall::ArchiveSeen {
            installation: machine(2),
            board: WireBoardId::new(fixture.board.as_str()).expect("board"),
            project: WireProjectId::new(first_project.as_str()).expect("project"),
            position: position(9, 99),
        },
        9,
    ) else {
        panic!("successor report");
    };
    assert_eq!(outcome(answer), ArchiveReportOutcome::AlreadyRecorded);
    assert_eq!(reports(&fixture), expected);
    assert_eq!(fixture.feed(), before);
    assert_eq!(fixture.ledger(), ledger);
    assert_eq!(
        outcome(fixture.report(1, 2, 3)),
        ArchiveReportOutcome::Recorded
    );
    let completed = reports(&fixture);
    assert_eq!(completed.first(), expected.first());
    let feed = fixture.feed();
    assert_eq!(
        outcome(fixture.report(0, 2, 10)),
        ArchiveReportOutcome::AlreadyRecorded
    );
    assert_eq!(reports(&fixture), completed);
    assert_eq!(fixture.feed(), feed);
    assert_eq!(fixture.ledger(), ledger);
}

#[test]
fn typed_report_refusals_keep_records_reports_feed_and_ledger_unchanged() {
    let mut fixture = ArchiveFixture::new();
    let board = fixture.board.clone();
    let project = fixture.projects.first().expect("project").clone();
    let ledger = fixture.ledger();
    let feed = fixture.feed();
    let record = fixture.board_record();
    let missing_board = BoardId::try_from(identity(1_801)).expect("board");
    assert_eq!(
        report(&mut fixture, &missing_board, &project),
        ArchiveReportOutcome::Refused(ArchiveReportRefusal::NotFound(missing_board.into()))
    );
    let missing_project = ProjectId::try_from(identity(1_802)).expect("project");
    assert_eq!(
        report(&mut fixture, &board, &missing_project),
        ArchiveReportOutcome::Refused(ArchiveReportRefusal::NotFound(missing_project.into()))
    );
    assert_eq!(
        report(&mut fixture, &board, &project),
        ArchiveReportOutcome::Refused(ArchiveReportRefusal::BoardNotArchiving {
            state: BoardState::Open
        })
    );
    assert_eq!(fixture.board_record(), record);
    assert_eq!(fixture.ledger(), ledger);
    assert_eq!(fixture.feed(), feed);
    assert!(reports(&fixture).is_empty());

    // A known project on a different existing board is distinct from NotFound.
    let other = BoardId::try_from(identity(1_803)).expect("other board");
    assert!(matches!(
        fixture.call(
            FleetCall::Import {
                bundle: Box::new(FleetImportBundle {
                    provenance: ImportProvenance {
                        installation: fixture.actor.installation.clone(),
                        machine: fixture.actor.machine.clone()
                    },
                    records: vec![FleetImportRecord::Board {
                        id: other.clone(),
                        workspace: fixture.workspace.clone(),
                        name: ResourceName::try_from("Other board".to_owned()).expect("name"),
                        description: Description::try_from("Other board".to_owned())
                            .expect("description"),
                        state: ImportedBoardState::Active,
                        created_at: at(0).0
                    }],
                })
            },
            0
        ),
        FleetResult::Imported(FleetImportResult::Imported { .. })
    ));
    let ledger = fixture.ledger();
    let feed = fixture.feed();
    assert_eq!(
        report(&mut fixture, &other, &project),
        ArchiveReportOutcome::Refused(ArchiveReportRefusal::ProjectNotOnBoard {
            project,
            board: other
        })
    );
    assert_eq!(fixture.ledger(), ledger);
    assert_eq!(fixture.feed(), feed);
    assert!(reports(&fixture).is_empty());
}

#[test]
fn finalization_failure_rolls_back_report_board_workspace_and_feed_together() {
    let mut fixture = ArchiveFixture::new();
    fixture.archive_workspace();
    assert_eq!(
        outcome(fixture.report(0, 1, 2)),
        ArchiveReportOutcome::Recorded
    );
    let report_rows = reports(&fixture);
    let board = fixture.board_record();
    let workspace = fixture.record("workspace", fixture.workspace.as_str());
    let feed = fixture.feed();
    let ledger = fixture.ledger();
    fixture.store.connection.execute_batch("CREATE TRIGGER reject_archive_feed BEFORE INSERT ON fleet_feed BEGIN SELECT RAISE(FAIL,'injected finalization failure'); END;").expect("failure seam");
    let input = WireInput::Call {
        from: ObjectAddress::Connection(machine(2)),
        call_id: ObjectCallId::new(identity(1_900)).expect("call"),
        call: Box::new(WireCall::Fleet(FleetCall::ArchiveSeen {
            installation: machine(2),
            board: WireBoardId::new(fixture.board.as_str()).expect("board"),
            project: WireProjectId::new(fixture.projects.get(1).expect("second").as_str())
                .expect("project"),
            position: position(2, 7),
        })),
    };
    assert!(matches!(
        fixture.store.run(input, at(3)),
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(reports(&fixture), report_rows);
    assert_eq!(fixture.board_record(), board);
    assert_eq!(
        fixture.record("workspace", fixture.workspace.as_str()),
        workspace
    );
    assert_eq!(fixture.feed(), feed);
    assert_eq!(fixture.ledger(), ledger);
    fixture
        .store
        .connection
        .execute_batch("DROP TRIGGER reject_archive_feed")
        .expect("remove seam");
    assert_eq!(
        outcome(fixture.report(1, 2, 3)),
        ArchiveReportOutcome::Recorded
    );
    assert_eq!(fixture.feed().len(), feed.len() + 2);
}

#[test]
fn corrupt_report_metadata_is_rejected_without_silently_counting_a_report() {
    for mutation in [
        "UPDATE archive_reports SET position_epoch='0'",
        "UPDATE archive_reports SET position_commit='not-a-number'",
        "UPDATE archive_reports SET installation='not-an-installation'",
        "UPDATE archive_reports SET reported_at='not-a-time'",
        "UPDATE archive_reports SET project='not-a-project'",
    ] {
        let mut fixture = ArchiveFixture::new();
        fixture.archive_workspace();
        fixture.report(0, 1, 2);
        fixture
            .store
            .connection
            .execute_batch(mutation)
            .expect("corrupt one field");
        let feed = fixture.feed();
        let board = fixture.board_record();
        let input = WireInput::Call {
            from: ObjectAddress::Connection(machine(2)),
            call_id: ObjectCallId::new(identity(1_901)).expect("call"),
            call: Box::new(WireCall::Fleet(FleetCall::ArchiveSeen {
                installation: machine(2),
                board: WireBoardId::new(fixture.board.as_str()).expect("board"),
                project: WireProjectId::new(fixture.projects.get(1).expect("second").as_str())
                    .expect("project"),
                position: position(2, 7),
            })),
        };
        assert!(
            matches!(
                fixture.store.run(input, at(3)),
                Err(CentralError::Store(StoreError::Corrupt { .. }))
            ),
            "{mutation}"
        );
        assert_eq!(fixture.feed(), feed);
        assert_eq!(fixture.board_record(), board);
    }
}
