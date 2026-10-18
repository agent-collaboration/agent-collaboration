#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;

use collaboration_central_core::*;
use fleet_admission::*;
use message_board::*;
use registry_support::{at, identity, machine, position, start};

mod archive_support;
use archive_support::*;

#[test]
fn released_attempts_never_reactivate_or_release_a_different_attempt() {
    let mut fixture = ArchiveFixture::new();
    let project = fixture.projects[0].clone();
    assert_eq!(
        reservation_result(fixture.call(reservation_call("releaseClaim", &project, 900), 1)),
        serde_json::json!({"kind":"claimReleased"})
    );
    let settled = serde_json::json!({"kind":"claimReserved","value":{"kind":"alreadySettled","value":{"outcome":"refused"}}});
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 900), 2)),
        settled
    );
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 901), 2)),
        serde_json::json!({"kind":"claimReserved","value":{"kind":"reserved"}})
    );
    fixture.call(reservation_call("releaseClaim", &project, 900), 3);
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 900), 3)),
        settled
    );
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 901), 3)),
        serde_json::json!({"kind":"claimReserved","value":{"kind":"reserved"}})
    );
    // A fresh transaction reads the durable disposition, not a cached result.
    fixture.call(reservation_call("releaseClaim", &project, 901), 4);
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 901), 5)),
        settled
    );
}

#[test]
fn releasing_the_last_reservation_finishes_archive_without_a_synthetic_report() {
    let mut fixture = ArchiveFixture::new();
    for project in fixture.projects.clone() {
        fixture.call(
            FleetCall::DirectoryUpdate(
                DirectoryUpdate::lease(
                    collaboration_replication_protocol::ProjectId::new(project.as_str())
                        .expect("project"),
                    collaboration_replication_protocol::DirectoryLease::Unheld,
                    LeaseVersion::new(
                        registry_support::epoch(1),
                        RegistryIncarnation::new(1).expect("incarnation"),
                        1,
                    )
                    .expect("version"),
                    start(),
                )
                .expect("unheld"),
            ),
            0,
        );
    }
    let project = fixture.projects[0].clone();
    assert_eq!(
        reservation_result(fixture.call(reservation_call("reserveClaim", &project, 902), 0)),
        serde_json::json!({"kind":"claimReserved","value":{"kind":"reserved"}})
    );
    fixture.archive_workspace();
    let ledger = fixture.ledger();
    let before = fixture.feed();
    fixture.call(reservation_call("releaseClaim", &project, 902), 3);
    assert!(
        matches!(fixture.board_record(), Record::Board(value) if value.state == BoardState::Archived && value.envelope.actor == fixture.actor && value.envelope.request.is_none() && value.envelope.router_time == at(3).0)
    );
    assert!(
        matches!(fixture.record("workspace", fixture.workspace.as_str()), Record::Workspace(value) if value.state == WorkspaceState::Archived && value.envelope.actor == fixture.actor && value.envelope.request.is_none() && value.envelope.router_time == at(3).0)
    );
    let finished = fixture.feed();
    assert_eq!(finished.len(), before.len() + 2);
    assert_eq!(fixture.ledger(), ledger);
    let reports: i64 = fixture
        .store
        .connection
        .query_row("SELECT count(*) FROM archive_reports", [], |row| row.get(0))
        .expect("reports");
    assert_eq!(reports, 0);
    fixture.call(reservation_call("releaseClaim", &project, 902), 5);
    fixture.call(reservation_call("reserveClaim", &project, 902), 6);
    assert_eq!(fixture.feed(), finished);
    assert_eq!(fixture.ledger(), ledger);
}

#[test]
fn archive_before_claim_returns_archived_without_fencing_or_granting() {
    use collaboration_replication_protocol::*;
    let mut fixture = ArchiveFixture::new();
    fixture.archive_workspace();
    let project = ProjectId::new(fixture.projects[0].as_str()).expect("project");
    let mut harness = registry_support::Harness::new();
    harness.stores.insert("fleet".into(), fixture.store);
    harness.enroll(1);
    harness.register(1, 10);
    let head = position(1, 0);
    let header = ImportSnapshotHeader {
        project: project.clone(),
        head,
        schema: SchemaIdentity::new(0, schema_fingerprint(&[]).expect("schema digest"))
            .expect("schema"),
        schema_steps: vec![],
    };
    let summary = ProjectSummary {
        status: "imported".into(),
        latest_record_at: start().0,
    };
    let upload = ProjectImportPage {
        digest: ImportDigestBuilder::new(&header)
            .expect("header")
            .finish(&summary)
            .expect("digest"),
        index: 0,
        page: SnapshotPage {
            project: project.clone(),
            position: head,
            schema: header.schema,
            schema_steps: vec![],
            table: String::new(),
            rows: vec![],
            next: None,
            as_of: start().0,
        },
        final_summary: Some(summary),
    };
    assert!(
        matches!(harness.call("registry", registry_support::admin(AdminRequest::ImportProject { project: project.clone(), upload: Box::new(upload) })), WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(ProjectImportAnswer::Imported { head: returned }))) if returned == head)
    );
    let call = WireCall::Registry(RegistryCall::Claim {
        attempt: RequestId::new(identity(950)).expect("attempt"),
        project: project.clone(),
        claimant: machine(1),
    });
    let expected = serde_json::json!({"kind":"refused","value":{"kind":"archived"}});
    for _ in 0..2 {
        let WireResult::Registry(RegistryResult::Claim(answer)) =
            harness.call("registry", call.clone())
        else {
            panic!("claim answer");
        };
        assert_eq!(serde_json::to_value(answer).expect("answer"), expected);
    }
    let WireResult::Copy(CopyResult::CopyStatus(Some(copy))) =
        harness.call("copy", WireCall::Copy(CopyCall::CopyStatus { project }))
    else {
        panic!("copy state");
    };
    assert_eq!(copy.fence, registry_support::epoch(1));
    assert_eq!(copy.holder, None);
    let leases: i64 = harness
        .store("registry")
        .connection
        .query_row("SELECT count(*) FROM _registry_leases", [], |row| {
            row.get(0)
        })
        .expect("leases");
    assert_eq!(leases, 0);
    let receipts: i64 = harness
        .store("copy")
        .connection
        .query_row("SELECT count(*) FROM _copy_transfers", [], |row| row.get(0))
        .expect("receipts");
    assert_eq!(receipts, 0);
}

#[test]
fn a_grant_at_the_unheld_timestamp_cannot_leave_archive_eligible_metadata() {
    use collaboration_replication_protocol::{
        DirectoryLease, ProjectId as WireProjectId, RequestId as WireRequestId,
    };
    let mut fixture = ArchiveFixture::new();
    for project in fixture.projects.clone() {
        fixture.call(
            FleetCall::DirectoryUpdate(
                DirectoryUpdate::lease(
                    WireProjectId::new(project.as_str()).expect("project"),
                    DirectoryLease::Unheld,
                    LeaseVersion::new(
                        registry_support::epoch(1),
                        RegistryIncarnation::new(1).expect("incarnation"),
                        1,
                    )
                    .expect("version"),
                    start(),
                )
                .expect("unheld"),
            ),
            0,
        );
    }
    let project = fixture.projects[0].clone();
    fixture.call(reservation_call("reserveClaim", &project, 960), 0);
    fixture.archive_workspace();
    let before = fixture.feed();
    fixture.call(
        FleetCall::DirectoryUpdate(
            DirectoryUpdate::claim_granted(
                WireProjectId::new(project.as_str()).expect("project"),
                WireRequestId::new(identity(960)).expect("attempt"),
                DirectoryLease::Held {
                    holder: machine(1),
                    state: collaboration_replication_protocol::LeaseState::Held,
                    expires_at: at(240).0,
                },
                LeaseVersion::new(
                    registry_support::epoch(2),
                    RegistryIncarnation::new(1).expect("incarnation"),
                    1,
                )
                .expect("version"),
                // Both real transitions may occur within one source clock tick.
                start(),
            )
            .expect("grant"),
        ),
        2,
    );
    let reservation: String = fixture
        .store
        .connection
        .query_row(
            "SELECT record FROM claim_reservations WHERE project=? AND attempt=?",
            [project.as_str(), identity(960).as_str()],
            |row| row.get(0),
        )
        .expect("reservation");
    let payload: String = fixture
        .store
        .connection
        .query_row(
            "SELECT record FROM directory_lease WHERE project=?",
            [project.as_str()],
            |row| row.get(0),
        )
        .expect("lease side");
    let reservation: ClaimReservation = serde_json::from_str(&reservation).expect("reservation");
    let projection: DirectoryUpdate = serde_json::from_str(&payload).expect("projection");
    let Record::Board(board) = fixture.board_record() else {
        panic!("board");
    };
    assert_eq!(
        board.state,
        BoardState::Archiving,
        "a granted claim still needs its holder's report; reservation={:?}; projection={projection:?}",
        reservation.state
    );
    assert_eq!(
        fixture.feed(),
        before,
        "grant settlement cannot finalize the archive"
    );
    assert!(matches!(
        projection,
        DirectoryUpdate::Lease {
            lease: DirectoryLease::Held { .. },
            ..
        }
    ));
}

#[test]
fn archive_reports_finalize_with_original_actors_without_a_request_ledger_entry() {
    let mut fixture = ArchiveFixture::new();
    fixture.archive_workspace();
    let ledger = fixture.ledger();
    let before_feed = fixture.feed();
    let original_board = fixture.board_record();
    let original_workspace = fixture.record("workspace", fixture.workspace.as_str());
    assert_report_answer(fixture.report(0, 1, 2), "recorded");
    assert_eq!(fixture.board_record(), original_board);
    assert_eq!(fixture.ledger(), ledger);
    assert_eq!(fixture.feed(), before_feed);

    assert_report_answer(fixture.report(1, 2, 3), "recorded");
    let Record::Board(mut expected_board) = original_board else {
        panic!("board");
    };
    let Record::Workspace(mut expected_workspace) = original_workspace else {
        panic!("workspace");
    };
    let board_position = before_feed
        .last()
        .expect("feed high water")
        .position
        .successor()
        .expect("position");
    let workspace_position = board_position.successor().expect("position");
    expected_board.state = BoardState::Archived;
    expected_board.envelope.position = RecordPosition::Fleet(board_position);
    expected_board.envelope.request = None;
    expected_board.envelope.router_time = at(3).0;
    expected_workspace.state = WorkspaceState::Archived;
    expected_workspace.envelope.position = RecordPosition::Fleet(workspace_position);
    expected_workspace.envelope.request = None;
    expected_workspace.envelope.router_time = at(3).0;
    assert_eq!(
        fixture.board_record(),
        Record::Board(expected_board.clone())
    );
    assert_eq!(
        fixture.record("workspace", fixture.workspace.as_str()),
        Record::Workspace(expected_workspace.clone())
    );
    let mut expected_feed = before_feed;
    expected_feed.extend([
        FleetFeedEntry {
            position: board_position,
            record: Record::Board(expected_board),
        },
        FleetFeedEntry {
            position: workspace_position,
            record: Record::Workspace(expected_workspace),
        },
    ]);
    assert_eq!(fixture.feed(), expected_feed);
    assert_eq!(fixture.ledger(), ledger);
    assert_report_answer(fixture.report(0, 2, 9), "alreadyRecorded");
    assert_eq!(fixture.feed(), expected_feed);
    assert_eq!(fixture.ledger(), ledger);
}
