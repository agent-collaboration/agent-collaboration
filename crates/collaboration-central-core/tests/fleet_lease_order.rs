#![allow(clippy::expect_used, clippy::panic)]
mod lease_order_support;
mod registry_support;

use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use lease_order_support::*;
use registry_support::*;

fn call(harness: &mut Harness, call: FleetCall) -> FleetResult {
    let WireResult::Fleet(result) = harness.call("fleet", WireCall::Fleet(call)) else {
        panic!("Fleet result");
    };
    result
}

fn update(harness: &mut Harness, update: DirectoryUpdate) -> LeaseUpdateResult {
    let FleetResult::LeaseUpdated(result) = call(harness, FleetCall::DirectoryUpdate(update))
    else {
        panic!("lease admission result");
    };
    result
}

fn seed_identity(harness: &mut Harness) {
    use fleet_admission::*;
    use message_board::{BoardId, Description, MachineLabel, ResourceName, ServiceId, WorkspaceId};
    let workspace = WorkspaceId::try_from(identity(800)).expect("workspace");
    let board = BoardId::try_from(identity(801)).expect("board");
    let bundle = FleetImportBundle {
        provenance: ImportProvenance {
            installation: ServiceId::try_from(machine(1).as_str().to_owned()).expect("service"),
            machine: MachineLabel::try_from("lease-order fixture".to_owned()).expect("machine"),
        },
        records: vec![
            FleetImportRecord::Workspace {
                id: workspace.clone(),
                name: ResourceName::try_from("Lease Workspace".to_owned()).expect("name"),
                description: Description::try_from("Ordering fixture".to_owned())
                    .expect("description"),
                repositories: vec![],
                created_at: start().0,
            },
            FleetImportRecord::Board {
                id: board.clone(),
                workspace,
                name: ResourceName::try_from("Lease Board".to_owned()).expect("name"),
                description: Description::try_from("Ordering fixture".to_owned())
                    .expect("description"),
                state: ImportedBoardState::Active,
                created_at: start().0,
            },
            FleetImportRecord::Directory {
                project: message_board::ProjectId::try_from(project().as_str().to_owned())
                    .expect("project"),
                board,
                title: ResourceName::try_from("Ordered Project".to_owned()).expect("title"),
                created_at: start().0,
            },
        ],
    };
    assert!(matches!(
        call(
            harness,
            FleetCall::Import {
                bundle: Box::new(bundle)
            }
        ),
        FleetResult::Imported(FleetImportResult::Imported { .. })
    ));
}

fn reserve(harness: &mut Harness, attempt: u64) {
    assert_eq!(
        call(
            harness,
            FleetCall::ReserveClaim {
                project: project(),
                attempt: request(attempt)
            }
        ),
        FleetResult::ClaimReserved(ReserveClaimResult::Reserved)
    );
}

fn reservation(harness: &mut Harness, attempt: u64) -> ClaimReservation {
    let record: String = harness
        .store("fleet")
        .connection
        .query_row(
            "SELECT record FROM claim_reservations WHERE project=? AND attempt=?",
            [project().as_str(), request(attempt).as_str()],
            |row| row.get(0),
        )
        .expect("reservation");
    serde_json::from_str(&record).expect("reservation record")
}

fn grant(update: DirectoryUpdate, attempt: u64) -> DirectoryUpdate {
    let DirectoryUpdate::Lease {
        project,
        lease,
        lease_version,
        as_of,
        ..
    } = update
    else {
        panic!("lease");
    };
    DirectoryUpdate::claim_granted(project, request(attempt), lease, lease_version, as_of)
        .expect("grant")
}

fn import_unheld(harness: &mut Harness) {
    seed_identity(harness);
    harness.enroll(1);
    harness.register(1, 10);
    let schema =
        SchemaIdentity::new(0, schema_fingerprint(&[]).expect("fingerprint")).expect("schema");
    let header = ImportSnapshotHeader {
        project: project(),
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
            project: project(),
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
                project: project(),
                upload: Box::new(upload)
            })
        ),
        WireResult::Registry(RegistryResult::Admin(AdminResult::ProjectImport(
            ProjectImportAnswer::Imported { .. }
        )))
    ));
}

#[test]
fn fenced_claim_repair_keeps_exact_settlement_identity_across_new_mutation_and_lost_ack() {
    let mut harness = Harness::new();
    import_unheld(&mut harness);
    let attempt = 2_200;
    let effects = harness
        .begin(
            "registry",
            WireCall::Registry(RegistryCall::Claim {
                attempt: request(attempt),
                project: project(),
                claimant: machine(1),
            }),
        )
        .1;
    let reservation_call = only_fleet(&effects);
    let reserved = reservation_call.answer(&mut harness);
    assert_eq!(
        reserved,
        FleetResult::ClaimReserved(ReserveClaimResult::Reserved)
    );
    let effects = reservation_call.deliver(&mut harness, reserved);
    let (call_id, fence_call) = effects
        .iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                call: WireCall::Copy(call),
                ..
            } => Some((call_id.clone(), call.clone())),
            _ => None,
        })
        .expect("real fence follows Reserved");
    let fence_answer = harness.call("copy", WireCall::Copy(fence_call));
    let effects = harness.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            call_id,
            result: Box::new(fence_answer),
            carry: None,
        },
    );
    assert!(effects.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Claimed { epoch: granted_epoch, .. })), .. } if *granted_epoch == epoch(2))));
    let offered = only_fleet(&effects);
    assert!(
        matches!(offered.update(), DirectoryUpdate::ClaimGranted { attempt: stored, .. } if stored == request(attempt))
    );
    call(
        &mut harness,
        FleetCall::AdvanceLeaseIncarnation { at_least: 1 },
    );
    let fenced = offered.answer(&mut harness);
    assert_eq!(
        fenced,
        FleetResult::LeaseUpdated(LeaseUpdateResult::Fenced {
            fence: RegistryIncarnation::new(2).expect("fence")
        })
    );
    assert_eq!(
        reservation(&mut harness, attempt).state,
        ClaimReservationState::Active
    );
    let acquisition = only_fleet(&offered.deliver(&mut harness, fenced));
    harness.now = RegistryTime(start().0 + chrono::TimeDelta::seconds(1));
    // A genuine renewal supersedes the offered grant projection while allocation is held.
    let effects = harness
        .begin(
            "registry",
            WireCall::Registry(RegistryCall::Renew {
                installation: machine(1),
                attempt: request(2_201),
                project: project(),
                epoch: epoch(2),
            }),
        )
        .1;
    assert!(fleet_dispatches(&effects).is_empty());
    let answer = acquisition.answer(&mut harness);
    let latest = only_fleet(&acquisition.deliver(&mut harness, answer));
    assert_eq!(version(&latest.update()).change(), 2);
    assert!(
        matches!(latest.update(), DirectoryUpdate::ClaimGranted { attempt: stored, lease: DirectoryLease::Held { expires_at, .. }, .. } if stored == request(attempt) && expires_at == harness.now.0 + chrono::TimeDelta::minutes(240))
    );
    assert_eq!(
        latest.answer(&mut harness),
        FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
    );
    assert_eq!(
        reservation(&mut harness, attempt).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted
        }
    );
    // Fleet committed, but its ACK is lost. Restart with Registry's durable debt intact.
    harness.restart_registry();
    harness.now = RegistryTime(start().0 + chrono::TimeDelta::seconds(6));
    let acquisition = only_fleet(&alarm(&mut harness));
    let answer = acquisition.answer(&mut harness);
    let repair = only_fleet(&acquisition.deliver(&mut harness, answer));
    assert!(
        matches!(repair.update(), DirectoryUpdate::ClaimGranted { attempt: stored, .. } if stored == request(attempt))
    );
    let answer = repair.answer(&mut harness);
    repair.deliver(&mut harness, answer);
    assert_eq!(publication_count(&mut harness), 0);
    let record: String = harness
        .store("registry")
        .connection
        .query_row(
            "SELECT record FROM _registry_transfers WHERE transfer_id=?",
            [request(attempt).as_str()],
            |row| row.get(0),
        )
        .expect("terminal transfer");
    let transfer: TransferRecord = serde_json::from_str(&record).expect("transfer");
    assert_eq!(transfer.fleet_cleanup, FleetCleanup::Acknowledged);
}

#[test]
fn delayed_unseen_incarnation_eight_is_fenced_after_another_project_advanced_the_witness() {
    let mut harness = Harness::new();
    assert_eq!(
        call(
            &mut harness,
            FleetCall::AdvanceLeaseIncarnation { at_least: 7 }
        ),
        FleetResult::LeaseIncarnationAdvanced(RegistryIncarnation::new(8).expect("incarnation"))
    );
    let delayed = lease_update(1, 8, 11, 1);
    // No P projection is installed. The outside witness, not P's maximum, protects P.
    call(
        &mut harness,
        FleetCall::AdvanceLeaseIncarnation { at_least: 3 },
    );
    assert_eq!(fence(&mut harness), 9);
    assert_eq!(
        update(&mut harness, delayed),
        LeaseUpdateResult::Fenced {
            fence: RegistryIncarnation::new(9).expect("fence")
        }
    );
    let rows: i64 = harness
        .store("fleet")
        .connection
        .query_row("SELECT count(*) FROM directory_lease", [], |row| row.get(0))
        .expect("lease rows");
    assert_eq!(rows, 0);
}

#[test]
fn tuple_admission_keeps_the_whole_projection_and_as_of_does_not_order_it() {
    let mut harness = Harness::new();
    call(
        &mut harness,
        FleetCall::AdvanceLeaseIncarnation { at_least: 8 },
    );
    let current = lease_update(2, 9, 11, 1);
    assert_eq!(
        update(&mut harness, current.clone()),
        LeaseUpdateResult::Applied
    );
    for older in [
        lease_update(2, 9, 10, 20),
        lease_update(2, 8, u64::MAX, 30),
        lease_update(1, u64::MAX, u64::MAX, 40),
    ] {
        let expected = if version(&older).incarnation().value() < 9 {
            LeaseUpdateResult::Fenced {
                fence: RegistryIncarnation::new(9).expect("fence"),
            }
        } else {
            LeaseUpdateResult::Older {
                stored: version(&current),
            }
        };
        assert_eq!(update(&mut harness, older), expected);
        assert_eq!(projection(&mut harness), current);
    }
    assert_eq!(
        update(&mut harness, current.clone()),
        LeaseUpdateResult::Duplicate
    );
    let conflict = lease_update(2, 9, 11, 50);
    assert_eq!(
        update(&mut harness, conflict),
        LeaseUpdateResult::Conflict {
            stored: version(&current)
        }
    );
    assert_eq!(projection(&mut harness), current);
    let newer = lease_update(3, 9, 1, 0);
    assert_eq!(
        update(&mut harness, newer.clone()),
        LeaseUpdateResult::Applied
    );
    assert_eq!(
        projection(&mut harness),
        newer,
        "higher epoch wins even with an older display timestamp"
    );
}

#[test]
fn fenced_claim_grant_applies_nothing_and_newer_lease_does_not_settle_another_attempt() {
    let mut harness = Harness::new();
    seed_identity(&mut harness);
    reserve(&mut harness, 2_000);
    reserve(&mut harness, 2_001);
    let initial = DirectoryUpdate::lease(
        project(),
        DirectoryLease::Unheld,
        LeaseVersion::new(
            epoch(1),
            RegistryIncarnation::new(1).expect("incarnation"),
            1,
        )
        .expect("version"),
        start(),
    )
    .expect("unheld");
    assert_eq!(
        update(&mut harness, initial.clone()),
        LeaseUpdateResult::Applied
    );
    call(
        &mut harness,
        FleetCall::AdvanceLeaseIncarnation { at_least: 1 },
    );
    assert_eq!(
        update(&mut harness, grant(lease_update(2, 1, 1, 0), 2_000)),
        LeaseUpdateResult::Fenced {
            fence: RegistryIncarnation::new(2).expect("fence")
        }
    );
    assert_eq!(projection(&mut harness), initial);
    assert_eq!(
        reservation(&mut harness, 2_000).state,
        ClaimReservationState::Active
    );
    assert_eq!(
        reservation(&mut harness, 2_001).state,
        ClaimReservationState::Active
    );

    let newer = lease_update(2, 2, 2, 1);
    update(&mut harness, newer.clone());
    assert_eq!(
        reservation(&mut harness, 2_000).state,
        ClaimReservationState::Active
    );
    // The exact grant can arrive after an even newer projection. Settlement is
    // still atomic and does not rewind that projection or touch the other attempt.
    assert_eq!(
        update(&mut harness, grant(lease_update(2, 2, 1, 0), 2_000)),
        LeaseUpdateResult::Older {
            stored: version(&newer)
        }
    );
    assert_eq!(projection(&mut harness), newer);
    assert_eq!(
        reservation(&mut harness, 2_000).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted
        }
    );
    assert_eq!(
        reservation(&mut harness, 2_001).state,
        ClaimReservationState::Active
    );
    assert_eq!(
        update(&mut harness, grant(newer.clone(), 2_000)),
        LeaseUpdateResult::Duplicate
    );
    assert_eq!(projection(&mut harness), newer);
}

#[test]
fn settlement_storage_failure_rolls_back_the_lease_projection_in_the_same_transaction() {
    let mut harness = Harness::new();
    seed_identity(&mut harness);
    reserve(&mut harness, 2_100);
    let initial = lease_update(1, 1, 1, 0);
    update(&mut harness, initial.clone());
    harness.store("fleet").connection.execute_batch("CREATE TRIGGER reject_settlement BEFORE UPDATE ON claim_reservations BEGIN SELECT RAISE(FAIL,'injected settlement failure'); END;").expect("failure seam");
    let input = harness.input(WireCall::Fleet(FleetCall::DirectoryUpdate(grant(
        lease_update(2, 1, 1, 1),
        2_100,
    ))));
    let now = harness.now;
    assert!(matches!(
        harness.store("fleet").run(input, now),
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(
        projection(&mut harness),
        initial,
        "the lease write preceding settlement must roll back"
    );
    assert_eq!(
        reservation(&mut harness, 2_100).state,
        ClaimReservationState::Active
    );
    harness
        .store("fleet")
        .connection
        .execute_batch("DROP TRIGGER reject_settlement")
        .expect("remove seam");
    assert_eq!(
        update(&mut harness, grant(lease_update(2, 1, 1, 1), 2_100)),
        LeaseUpdateResult::Applied
    );
    assert_eq!(
        reservation(&mut harness, 2_100).state,
        ClaimReservationState::Settled {
            outcome: ClaimReservationOutcome::Granted
        }
    );
}

#[test]
fn invalid_incoming_digest_and_corrupt_stored_versions_fail_without_coercion() {
    let mut harness = Harness::new();
    let original = lease_update(1, 1, 1, 0);
    update(&mut harness, original.clone());
    let mut invalid = lease_update(1, 1, 2, 1);
    if let DirectoryUpdate::Lease { lease, .. } = &mut invalid {
        *lease = DirectoryLease::Unheld;
    }
    let input = harness.input(WireCall::Fleet(FleetCall::DirectoryUpdate(invalid)));
    let now = harness.now;
    assert!(
        matches!(harness.store("fleet").run(input, now), Err(CentralError::InvalidInput { message }) if message == "lease content digest mismatch")
    );
    assert_eq!(projection(&mut harness), original);
    let mut corrupt = serde_json::to_value(original).expect("stored update");
    *corrupt
        .get_mut("value")
        .and_then(|value| value.get_mut("lease_version"))
        .and_then(|version| version.get_mut("change"))
        .expect("actual stored change field") = serde_json::json!(0);
    harness
        .store("fleet")
        .connection
        .execute(
            "UPDATE directory_lease SET record=?",
            [serde_json::to_string(&corrupt).expect("corrupt record")],
        )
        .expect("corrupt stored version");
    let input = harness.input(WireCall::Fleet(FleetCall::DirectoryUpdate(lease_update(
        1, 1, 3, 2,
    ))));
    assert!(matches!(
        harness.store("fleet").run(input, now),
        Err(CentralError::Store(StoreError::Corrupt { .. }))
    ));
}
