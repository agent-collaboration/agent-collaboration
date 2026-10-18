#![allow(clippy::expect_used, clippy::panic)]
mod archive_support;
mod lease_order_support;
mod registry_support;

use archive_support::ArchiveFixture;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use lease_order_support::{alarm, fleet_dispatches, only_fleet};
use message_board::{BoardState, Record, WorkspaceState};
use registry_support::*;

struct ClaimArchiveFixture {
    harness: Harness,
    archive: ArchiveFixture,
    project: ProjectId,
}

impl ClaimArchiveFixture {
    fn new() -> Self {
        let mut archive = ArchiveFixture::new();
        for project in archive.projects.clone() {
            archive.call(
                FleetCall::DirectoryUpdate(
                    DirectoryUpdate::lease(
                        ProjectId::new(project.as_str()).expect("project"),
                        DirectoryLease::Unheld,
                        LeaseVersion::new(
                            epoch(1),
                            RegistryIncarnation::new(1).expect("incarnation"),
                            1,
                        )
                        .expect("version"),
                        start(),
                    )
                    .expect("Unheld projection"),
                ),
                0,
            );
        }
        let project =
            ProjectId::new(archive.projects.first().expect("first").as_str()).expect("project");
        let mut harness = Harness::new();
        harness.stores.insert(
            "fleet".into(),
            std::mem::replace(
                &mut archive.store,
                registry_support::sql_support::SqlTxFake::blank(),
            ),
        );
        for installation in [1, 2] {
            harness.enroll(installation);
            harness.register(installation, 10);
        }
        let schema =
            SchemaIdentity::new(0, schema_fingerprint(&[]).expect("fingerprint")).expect("schema");
        let header = ImportSnapshotHeader {
            project: project.clone(),
            head: position(1, 0),
            schema: schema.clone(),
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
                position: header.head,
                schema,
                schema_steps: vec![],
                table: String::new(),
                rows: vec![],
                next: None,
                as_of: start().0,
            },
            final_summary: Some(summary),
        };
        assert!(matches!(
            harness.call(
                "registry",
                admin(AdminRequest::ImportProject {
                    project: project.clone(),
                    upload: Box::new(upload)
                })
            ),
            WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
                ProjectImportAnswer::Imported { .. }
            )))
        ));
        Self {
            harness,
            archive,
            project,
        }
    }

    fn claim(&self, attempt: u64) -> WireCall {
        WireCall::Registry(RegistryCall::Claim {
            attempt: request(attempt),
            project: self.project.clone(),
            claimant: machine(1),
        })
    }

    fn archive_workspace(&mut self) {
        // Move the same real SQL connection between fixtures, never replace the receiver.
        let fleet = self.harness.store("fleet");
        std::mem::swap(&mut self.archive.store, fleet);
        self.archive.archive_workspace();
        std::mem::swap(&mut self.archive.store, fleet);
    }

    fn board_state(&mut self) -> BoardState {
        let record: String = self
            .harness
            .store("fleet")
            .connection
            .query_row(
                "SELECT payload FROM fleet_records WHERE kind='board' AND id=?",
                [self.archive.board.as_str()],
                |row| row.get(0),
            )
            .expect("board");
        let Record::Board(board) = serde_json::from_str(&record).expect("record") else {
            panic!("board");
        };
        board.state
    }

    fn reservation(&mut self, attempt: u64) -> ClaimReservation {
        let record: String = self
            .harness
            .store("fleet")
            .connection
            .query_row(
                "SELECT record FROM claim_reservations WHERE project=? AND attempt=?",
                [self.project.as_str(), request(attempt).as_str()],
                |row| row.get(0),
            )
            .expect("reservation");
        serde_json::from_str(&record).expect("reservation")
    }

    fn transfer(&mut self, attempt: u64) -> TransferRecord {
        let record: String = self
            .harness
            .store("registry")
            .connection
            .query_row(
                "SELECT record FROM _registry_transfers WHERE transfer_id=?",
                [request(attempt).as_str()],
                |row| row.get(0),
            )
            .expect("transfer");
        serde_json::from_str(&record).expect("transfer")
    }

    fn copy_call(&mut self, effects: &[WireEffect], deliver: bool) -> Vec<WireEffect> {
        let (call_id, call) = effects
            .iter()
            .find_map(|effect| match effect {
                WireEffect::Dispatch {
                    call_id,
                    call: WireCall::Copy(call),
                    ..
                } => Some((call_id.clone(), call.clone())),
                _ => None,
            })
            .expect("copy dispatch");
        let result = self.harness.call("copy", WireCall::Copy(call));
        if !deliver {
            return Vec::new();
        }
        self.harness.run(
            "registry",
            WireInput::CallResult {
                source: CallerKind::Registry,
                call_id,
                result: Box::new(result),
                carry: None,
            },
        )
    }
}

#[path = "claim_archive_repair/unavailable.rs"]
mod unavailable;

#[test]
fn reservation_committed_before_archive_survives_lost_answer_and_registry_restart() {
    let mut fixture = ClaimArchiveFixture::new();
    let claim = fixture.claim(3_000);
    let (_, effects) = fixture.harness.begin("registry", claim.clone());
    let reserve = only_fleet(&effects);
    let answer = reserve.answer(&mut fixture.harness);
    assert_eq!(
        answer,
        FleetResult::ClaimReserved(ReserveClaimResult::Reserved)
    );
    fixture.archive_workspace();
    assert_eq!(fixture.board_state(), BoardState::Archiving);
    fixture.harness.restart_registry();
    let granted = fixture.harness.call("registry", claim.clone());
    assert_eq!(
        granted,
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed {
            epoch: epoch(2),
            expires_at: at(240).0,
            start: position(1, 0)
        }))
    );
    assert_eq!(
        fixture.reservation(3_000).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted
        }
    );
    assert_eq!(
        fixture.board_state(),
        BoardState::Archiving,
        "held grant still needs its holder report"
    );
    reserve.deliver(&mut fixture.harness, answer);
    fixture.harness.now = at(1);
    assert_eq!(fixture.harness.call("registry", claim), granted);
}

#[test]
fn fencing_is_not_abandoned_on_timeout_and_a_committed_fence_replays_after_restart() {
    let mut fixture = ClaimArchiveFixture::new();
    let claim = fixture.claim(3_010);
    let (_, effects) = fixture.harness.begin("registry", claim.clone());
    let reserve = only_fleet(&effects);
    let answer = reserve.answer(&mut fixture.harness);
    let fence = reserve.deliver(&mut fixture.harness, answer);
    fixture.archive_workspace();
    fixture.harness.now = at(20);
    fixture.harness.restart_registry();
    assert!(
        fleet_dispatches(&alarm(&mut fixture.harness))
            .iter()
            .all(|call| !matches!(call.call, FleetCall::ReleaseClaim { .. }))
    );
    assert_eq!(
        fixture.reservation(3_010).state,
        ClaimReservationState::Active
    );
    assert!(matches!(
        fixture.transfer(3_010).state,
        TransferState::Fencing { .. }
    ));
    assert_eq!(fixture.board_state(), BoardState::Archiving);
    // Copy commits the real fence; its answer is then lost before Registry grant.
    fixture.copy_call(&fence, false);
    let WireResult::Copy(CopyResult::CopyStatus(Some(copy))) = fixture.harness.call(
        "copy",
        WireCall::Copy(CopyCall::CopyStatus {
            project: fixture.project.clone(),
        }),
    ) else {
        panic!("copy");
    };
    assert_eq!(copy.fence, epoch(2));
    assert_eq!(copy.holder, Some(machine(1)));
    assert!(matches!(
        fixture.transfer(3_010).state,
        TransferState::Fencing { .. }
    ));
    fixture.harness.restart_registry();
    let granted = fixture.harness.call("registry", claim.clone());
    assert_eq!(
        granted,
        WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed {
            epoch: epoch(2),
            expires_at: at(260).0,
            start: position(1, 0)
        }))
    );
    fixture.harness.now = at(21);
    assert_eq!(fixture.harness.call("registry", claim), granted);
    assert_eq!(fixture.board_state(), BoardState::Archiving);
}

#[test]
fn refused_cleanup_repairs_dropped_release_and_lost_ack_across_restarts_without_synthetic_report() {
    let mut fixture = ClaimArchiveFixture::new();
    let (_, effects) = fixture.harness.begin("registry", fixture.claim(3_020));
    let reserve = only_fleet(&effects);
    let answer = reserve.answer(&mut fixture.harness);
    let fence = reserve.deliver(&mut fixture.harness, answer);
    fixture.harness.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            transfer_id: transfer(3_021),
            new_epoch: epoch(4),
            holder: machine(2),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 0),
            },
            target_schema_max: 10,
        }),
    );
    let status = fixture.copy_call(&fence, true);
    let refused = fixture.copy_call(&status, true);
    assert!(refused.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(SessionRefusal::AlreadyHeld { holder }))), .. } if *holder == machine(2))));
    let release = only_fleet(&refused);
    assert!(
        matches!(release.call, FleetCall::ReleaseClaim { ref project, ref attempt } if *project == fixture.project && *attempt == request(3_020))
    );
    assert!(matches!(
        fixture.transfer(3_020).fleet_cleanup,
        FleetCleanup::Owed(ClaimCleanupDebt::Refused { .. })
    ));
    fixture.archive_workspace();
    assert_eq!(fixture.board_state(), BoardState::Archiving);
    assert_eq!(
        fixture.reservation(3_020).state,
        ClaimReservationState::Active
    );
    let before = fixture
        .harness
        .store("fleet")
        .connection
        .query_row("SELECT count(*) FROM fleet_feed", [], |row| {
            row.get::<_, i64>(0)
        })
        .expect("feed count");
    // First dispatch is lost entirely. A new actor's alarm must retain the debt.
    fixture.harness.restart_registry();
    fixture.harness.now = RegistryTime(start().0 + chrono::TimeDelta::seconds(5));
    let retry = only_fleet(&alarm(&mut fixture.harness));
    assert_eq!(retry.call, release.call);
    assert_eq!(
        retry.answer(&mut fixture.harness),
        FleetResult::ClaimReleased
    );
    assert_eq!(
        fixture.reservation(3_020).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Refused
        }
    );
    assert_eq!(fixture.board_state(), BoardState::Archived);
    // Fleet committed finalization, but Registry loses its acknowledgement too.
    fixture.harness.restart_registry();
    fixture.harness.now = RegistryTime(start().0 + chrono::TimeDelta::seconds(10));
    let retry = only_fleet(&alarm(&mut fixture.harness));
    let answer = retry.answer(&mut fixture.harness);
    retry.deliver(&mut fixture.harness, answer);
    assert_eq!(
        fixture.transfer(3_020).fleet_cleanup,
        FleetCleanup::Acknowledged
    );
    assert!(fleet_dispatches(&alarm(&mut fixture.harness)).is_empty());
    let fleet = fixture.harness.store("fleet");
    let after: i64 = fleet
        .connection
        .query_row("SELECT count(*) FROM fleet_feed", [], |row| row.get(0))
        .expect("feed count");
    assert_eq!(
        after,
        before + 2,
        "completion emits each board/workspace record exactly once"
    );
    let report_count: i64 = fleet
        .connection
        .query_row("SELECT count(*) FROM archive_reports", [], |row| row.get(0))
        .expect("reports");
    assert_eq!(report_count, 0);
    let workspace: String = fleet
        .connection
        .query_row(
            "SELECT payload FROM fleet_records WHERE kind='workspace'",
            [],
            |row| row.get(0),
        )
        .expect("workspace");
    assert!(
        matches!(serde_json::from_str::<Record>(&workspace).expect("record"), Record::Workspace(workspace) if workspace.state == WorkspaceState::Archived && workspace.envelope.request.is_none() && workspace.envelope.actor == fixture.archive.actor)
    );
}
