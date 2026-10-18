#![allow(clippy::expect_used, clippy::panic)]
mod lease_order_support;
mod registry_support;

use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use lease_order_support::*;
use registry_support::*;

#[test]
fn renewal_counter_attempt_and_publication_debt_commit_or_roll_back_together() {
    let mut harness = Harness::setup();
    let original: String = harness
        .store("registry")
        .connection
        .query_row("SELECT record FROM _registry_leases", [], |row| row.get(0))
        .expect("original lease");
    harness.store("registry").connection.execute_batch("CREATE TRIGGER reject_publication BEFORE INSERT ON _registry_lease_publications BEGIN SELECT RAISE(FAIL,'injected publication failure'); END;").expect("failure seam");
    harness.now = at(1);
    let input = harness.input(WireCall::Registry(RegistryCall::Renew {
        installation: machine(1),
        attempt: request(990),
        project: project(),
        epoch: epoch(1),
    }));
    let now = harness.now;
    assert!(matches!(
        harness.store("registry").run(input, now),
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    let after: String = harness
        .store("registry")
        .connection
        .query_row("SELECT record FROM _registry_leases", [], |row| row.get(0))
        .expect("rolled back lease");
    assert_eq!(after, original);
    let attempts: i64 = harness
        .store("registry")
        .connection
        .query_row(
            "SELECT count(*) FROM _registry_renewal_attempts WHERE attempt=?",
            [request(990).as_str()],
            |row| row.get(0),
        )
        .expect("attempt count");
    assert_eq!(attempts, 0);
    assert_eq!(publication_count(&mut harness), 0);
    harness
        .store("registry")
        .connection
        .execute_batch("DROP TRIGGER reject_publication")
        .expect("remove seam");
    let offer = only_fleet(&begin_renew(&mut harness, 990));
    assert_eq!(version(&offer.update()).change(), 2);
}

#[test]
fn lost_allocation_answer_and_crash_before_installation_keep_the_run_gated() {
    let mut harness = Harness::setup();
    harness.restart_registry();
    harness.now = at(1);
    let lost = only_fleet(&begin_renew(&mut harness, 1_000));
    assert_eq!(lost.allocation_floor(), 1);
    let lost_answer = lost.answer(&mut harness);
    assert_eq!(
        lost_answer,
        FleetResult::LeaseIncarnationAdvanced(RegistryIncarnation::new(2).expect("incarnation"))
    );
    assert_eq!(fence(&mut harness), 2);
    assert!(
        fleet_dispatches(&alarm(&mut harness)).is_empty(),
        "no retry before its deadline"
    );

    harness.now = RegistryTime(harness.now.0 + chrono::TimeDelta::seconds(5));
    let retry = only_fleet(&alarm(&mut harness));
    assert_ne!(retry.call_id, lost.call_id);
    assert_eq!(retry.allocation_floor(), 1);
    let retry_answer = retry.answer(&mut harness);
    assert_eq!(fence(&mut harness), 3);
    assert!(
        lost.deliver(&mut harness, lost_answer).is_empty(),
        "the delayed first answer cannot install incarnation 2"
    );

    // Fleet committed 3, but this physical actor dies before storing the answer.
    harness.restart_registry();
    assert!(retry.deliver(&mut harness, retry_answer).is_empty());
    let next = only_fleet(&alarm(&mut harness));
    assert_eq!(next.allocation_floor(), 1);
    let next_answer = next.answer(&mut harness);
    let publication = only_fleet(&next.deliver(&mut harness, next_answer));
    assert_eq!(version(&publication.update()).incarnation().value(), 4);
    assert_eq!(
        publication.answer(&mut harness),
        FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
    );
}

#[test]
fn registry_restored_to_three_gets_ten_from_fleets_retained_nine() {
    let mut harness = Harness::setup();
    for attempt in [1_010, 1_011] {
        harness.restart_registry();
        harness.now = at(attempt - 1_009);
        harness.renew(1, attempt as u64, 1);
    }
    assert_eq!(fence(&mut harness), 3);
    let remembered: String = harness
        .store("registry")
        .connection
        .query_row("SELECT record FROM _registry_lease_run", [], |row| {
            row.get(0)
        })
        .expect("remembered incarnation");
    let lease: String = harness
        .store("registry")
        .connection
        .query_row("SELECT record FROM _registry_leases", [], |row| row.get(0))
        .expect("remembered lease");
    for attempt in 1_012..=1_017 {
        harness.restart_registry();
        harness.now = at(attempt - 1_009);
        harness.renew(1, attempt as u64, 1);
    }
    assert_eq!(fence(&mut harness), 9);
    let newer = projection(&mut harness);
    // Restore Registry alone to an actual earlier committed image.
    harness
        .store("registry")
        .connection
        .execute("UPDATE _registry_lease_run SET record=?", [remembered])
        .expect("restore run");
    harness
        .store("registry")
        .connection
        .execute("UPDATE _registry_leases SET record=?", [lease])
        .expect("restore lease");
    harness.restart_registry();
    harness.now = at(9);
    let allocation = only_fleet(&begin_renew(&mut harness, 1_018));
    assert_eq!(allocation.allocation_floor(), 3);
    let answer = allocation.answer(&mut harness);
    assert_eq!(fence(&mut harness), 10);
    let publication = only_fleet(&allocation.deliver(&mut harness, answer));
    let current = publication.update();
    assert_eq!(version(&current).incarnation().value(), 10);
    assert!(
        version(&current).change() <= version(&newer).change(),
        "restored change is not used to infer recovery"
    );
    assert_eq!(
        publication.answer(&mut harness),
        FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
    );
    assert_eq!(projection(&mut harness), current);
}

#[test]
fn reordering_changes_and_replaying_attempts_never_allocate_or_republish() {
    let mut harness = Harness::setup();
    let mut offers = Vec::new();
    for change in 2..=11 {
        harness.now = at(change);
        offers.push(only_fleet(&begin_renew(
            &mut harness,
            1_100 + change as u64,
        )));
    }
    let newer = offers.pop().expect("11");
    let older = offers.pop().expect("10");
    assert_eq!(version(&newer.update()).change(), 11);
    assert_eq!(version(&older.update()).change(), 10);
    assert_eq!(
        newer.answer(&mut harness),
        FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
    );
    let older_answer = older.answer(&mut harness);
    assert_eq!(
        older_answer,
        FleetResult::LeaseUpdated(LeaseUpdateResult::Older {
            stored: version(&newer.update())
        })
    );
    assert!(older.deliver(&mut harness, older_answer).is_empty());
    assert_eq!(publication_count(&mut harness), 1);
    let before = projection(&mut harness);
    let fence_before = fence(&mut harness);
    for _ in 0..10 {
        let effects = begin_renew(&mut harness, 1_110);
        assert!(fleet_dispatches(&effects).is_empty());
        assert!(effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Return {
                result: WireResult::Registry(RegistryResult::Renew(SessionAnswer::Renewed {
                    replayed: true,
                    ..
                })),
                ..
            }
        )));
    }
    assert_eq!(projection(&mut harness), before);
    assert_eq!(fence(&mut harness), fence_before);
    assert!(
        newer
            .deliver(
                &mut harness,
                FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
            )
            .is_empty()
    );
    assert_eq!(publication_count(&mut harness), 0);
}

#[test]
fn obsolete_fenced_conflict_and_success_answers_cannot_clear_or_restart_newer_work() {
    let mut harness = Harness::setup();
    harness.now = at(1);
    let old = only_fleet(&begin_renew(&mut harness, 1_200));
    harness.now = at(2);
    let current = only_fleet(&begin_renew(&mut harness, 1_201));
    let before = fence(&mut harness);
    for answer in [
        LeaseUpdateResult::Applied,
        LeaseUpdateResult::Duplicate,
        LeaseUpdateResult::Older {
            stored: version(&current.update()),
        },
        LeaseUpdateResult::Conflict {
            stored: version(&old.update()),
        },
        LeaseUpdateResult::Fenced {
            fence: RegistryIncarnation::new(9).expect("fence"),
        },
    ] {
        assert!(
            old.deliver(&mut harness, FleetResult::LeaseUpdated(answer))
                .is_empty()
        );
        assert_eq!(publication_count(&mut harness), 1);
        assert_eq!(fence(&mut harness), before);
    }
    // Recover the actual current offer, then replay its old Fenced reply again.
    harness.call(
        "fleet",
        WireCall::Fleet(FleetCall::AdvanceLeaseIncarnation { at_least: 8 }),
    );
    let fenced = current.answer(&mut harness);
    let acquire = only_fleet(&current.deliver(&mut harness, fenced.clone()));
    assert_eq!(acquire.allocation_floor(), 9);
    let answer = acquire.answer(&mut harness);
    let replacement = only_fleet(&acquire.deliver(&mut harness, answer));
    assert_eq!(version(&replacement.update()).incarnation().value(), 10);
    assert!(current.deliver(&mut harness, fenced).is_empty());
    assert!(
        current
            .deliver(
                &mut harness,
                FleetResult::LeaseUpdated(LeaseUpdateResult::Conflict {
                    stored: version(&current.update())
                })
            )
            .is_empty()
    );
    assert_eq!(fence(&mut harness), 10);
    assert_eq!(publication_count(&mut harness), 1);
    let answer = replacement.answer(&mut harness);
    replacement.deliver(&mut harness, answer);
    assert_eq!(publication_count(&mut harness), 0);
}

#[test]
fn fenced_and_conflicting_current_offers_rebuild_from_current_rows() {
    for conflict in [false, true] {
        let mut harness = Harness::setup();
        harness.now = at(1);
        let offered = only_fleet(&begin_renew(&mut harness, 1_300));
        let answer = if conflict {
            // Equal key, different valid content models Fleet-only rollback/collision.
            let other = lease_update(1, 1, version(&offered.update()).change(), 5);
            harness.call("fleet", WireCall::Fleet(FleetCall::DirectoryUpdate(other)));
            offered.answer(&mut harness)
        } else {
            harness.call(
                "fleet",
                WireCall::Fleet(FleetCall::AdvanceLeaseIncarnation { at_least: 8 }),
            );
            offered.answer(&mut harness)
        };
        assert!(matches!(
            answer,
            FleetResult::LeaseUpdated(
                LeaseUpdateResult::Fenced { .. } | LeaseUpdateResult::Conflict { .. }
            )
        ));
        let acquire = only_fleet(&offered.deliver(&mut harness, answer));
        harness.now = RegistryTime(harness.now.0 + chrono::TimeDelta::seconds(1));
        let latest_time = harness.now;
        assert!(
            fleet_dispatches(&begin_renew(&mut harness, 1_301)).is_empty(),
            "new mutation stays gated"
        );
        let answer = acquire.answer(&mut harness);
        let current = only_fleet(&acquire.deliver(&mut harness, answer));
        let update = current.update();
        assert_eq!(version(&update).change(), 3);
        assert!(version(&update).incarnation() > version(&offered.update()).incarnation());
        assert!(
            matches!(&update, DirectoryUpdate::Lease { as_of, lease: DirectoryLease::Held { expires_at, .. }, .. } if *as_of == latest_time && *expires_at == latest_time.0 + chrono::TimeDelta::minutes(240))
        );
        let answer = current.answer(&mut harness);
        assert_eq!(
            answer,
            FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
        );
        current.deliver(&mut harness, answer);
        assert_eq!(projection(&mut harness), update);
        assert_eq!(publication_count(&mut harness), 0);
    }
}

#[test]
fn unavailable_fleet_leaves_publication_gated_but_renewal_and_replay_authoritative() {
    let mut harness = Harness::setup();
    let before = projection(&mut harness);
    harness.restart_registry();
    harness.now = at(1);
    let allocation = only_fleet(&begin_renew(&mut harness, 1_400));
    assert_eq!(allocation.allocation_floor(), 1);
    // No answer is delivered: the same durable lease decision remains available.
    harness.now = at(2);
    let effects = begin_renew(&mut harness, 1_401);
    assert!(
        fleet_dispatches(&effects)
            .iter()
            .all(|call| matches!(call.call, FleetCall::AdvanceLeaseIncarnation { .. }))
    );
    assert!(effects.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Renew(SessionAnswer::Renewed { replayed: false, expires_at, .. })), .. } if *expires_at == at(242).0)));
    for _ in 0..3 {
        assert!(fleet_dispatches(&begin_renew(&mut harness, 1_400)).is_empty());
    }
    assert_eq!(projection(&mut harness), before);
    harness.now = at(3);
    let retry = only_fleet(&alarm(&mut harness));
    let answer = retry.answer(&mut harness);
    let update = only_fleet(&retry.deliver(&mut harness, answer));
    assert_eq!(version(&update.update()).change(), 3);
    assert_eq!(
        update.answer(&mut harness),
        FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)
    );
}
