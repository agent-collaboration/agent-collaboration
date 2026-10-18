#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;

use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

fn fail_copy(
    h: &mut Harness,
    effects: Vec<WireEffect>,
    reason: CopyUnavailableReason,
) -> Vec<WireEffect> {
    let call_id = effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                to: ObjectAddress::Copy(_),
                ..
            } => Some(call_id),
            _ => None,
        })
        .expect("copy dispatch");
    h.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            carry: None,
            call_id,
            result: Box::new(WireResult::Copy(CopyResult::Unavailable(reason))),
        },
    )
}

fn records(h: &mut Harness, table: &str) -> Vec<String> {
    let sql = format!("SELECT record FROM {table} ORDER BY record");
    let mut query = h
        .store("registry")
        .connection
        .prepare(&sql)
        .expect("records");
    query
        .query_map([], |row| row.get::<_, String>(0))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("rows")
}

fn reasons() -> [CopyUnavailableReason; 2] {
    [
        CopyUnavailableReason::StorageFull,
        CopyUnavailableReason::MigrationPending {
            kind: BookkeepingKind::Copy,
            version: 3,
        },
    ]
}

#[test]
fn first_creation_retains_its_exact_pending_lease_and_retries_the_same_epoch() {
    for reason in reasons() {
        let mut h = Harness::new();
        h.enroll(1);
        let call = WireCall::Registry(RegistryCall::ReserveFirstEpoch {
            project: project(),
            source: ReservationSource::FleetAcceptance {
                installation: machine(1),
            },
        });
        let (_, effects) = h.begin("registry", call.clone());
        let before = records(&mut h, "_registry_reservations");
        assert_eq!(before.len(), 1);
        let effects = fail_copy(&mut h, effects, reason.clone());
        assert!(matches!(effects.as_slice(), [WireEffect::Return {
            result: WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::Unavailable {
                subject: BlockedSubject::Project { project: target }, reason: actual,
            })), ..
        }] if target == &project() && actual == &reason));
        assert_eq!(records(&mut h, "_registry_reservations"), before);
        assert!(records(&mut h, "_registry_leases").is_empty());
        h.restart_registry();
        h.now = at(5);
        assert_eq!(
            h.call("registry", call.clone()),
            WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease {
                epoch: epoch(1),
                expires_at: at(240).0
            }))
        );
        assert_eq!(
            h.call("registry", call),
            WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease {
                epoch: epoch(1),
                expires_at: at(240).0
            }))
        );
    }
}

#[test]
fn preview_unavailability_has_project_identity_and_does_not_make_a_transfer() {
    let mut h = Harness::setup();
    let call = admin(AdminRequest::TakeoverPreview { project: project() });
    let (_, effects) = h.begin("registry", call.clone());
    let effects = fail_copy(&mut h, effects, CopyUnavailableReason::StorageFull);
    assert!(matches!(effects.as_slice(), [WireEffect::Return {
        result: WireResult::Registry(RegistryResult::Admin(AdminResult::Unavailable {
            subject: BlockedSubject::Project { project: target }, reason: CopyUnavailableReason::StorageFull,
        })), ..
    }] if target == &project()));
    assert!(records(&mut h, "_registry_transfers").is_empty());
    assert!(matches!(
        h.call("registry", call),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Preview(_)))
    ));
    let (_, effects) = h.begin(
        "registry",
        admin(AdminRequest::Takeover {
            transfer_id: transfer(9),
            project: project(),
            to_installation: machine(2),
            reason: "rejected preview".into(),
            confirm_token: "unknown".into(),
        }),
    );
    let effects = fail_copy(&mut h, effects, CopyUnavailableReason::StorageFull);
    assert!(matches!(effects.as_slice(), [WireEffect::Return {
        result: WireResult::Registry(RegistryResult::Admin(AdminResult::Unavailable {
            subject: BlockedSubject::Project { project: target }, ..
        })), ..
    }] if target == &project()));
    assert!(records(&mut h, "_registry_transfers").is_empty());
}

#[test]
fn unavailable_fence_keeps_transfer_epoch_and_lease_then_same_request_grants_once() {
    for reason in reasons() {
        for committed in [false, true] {
            let mut h = Harness::setup();
            let call = h.takeover_call(1);
            let (_, effects) = h.begin("registry", call.clone());
            if committed {
                let fence = effects
                    .iter()
                    .find_map(|effect| match effect {
                        WireEffect::Dispatch { call, .. } => Some(call.clone()),
                        _ => None,
                    })
                    .expect("fence");
                assert!(
                    matches!(h.call("copy", fence), WireResult::Copy(CopyResult::Fence(FenceResult::Fenced { new_epoch, .. })) if new_epoch == epoch(2))
                );
            }
            let transfers = records(&mut h, "_registry_transfers");
            let leases = records(&mut h, "_registry_leases");
            assert!(transfers[0].contains("fencing"));
            let effects = fail_copy(&mut h, effects, reason.clone());
            assert!(matches!(effects.as_slice(), [WireEffect::Return {
                result: WireResult::Registry(RegistryResult::Admin(AdminResult::Unavailable {
                    subject: BlockedSubject::Transfer { transfer: actual }, reason: actual_reason,
                })), ..
            }] if actual == &transfer(1) && actual_reason == &reason));
            assert_eq!(records(&mut h, "_registry_transfers"), transfers);
            assert_eq!(records(&mut h, "_registry_leases"), leases);
            h.restart_registry();
            for _ in 0..2 {
                assert!(
                    matches!(h.call("registry", call.clone()), WireResult::Registry(RegistryResult::Admin(AdminResult::Granted { new_epoch, start })) if new_epoch == epoch(2) && start == position(1, 0))
                );
            }
            assert_eq!(records(&mut h, "_registry_transfers").len(), 1);
        }
    }
}

#[test]
fn unavailable_holder_release_keeps_fencing_and_the_same_release_finishes() {
    for reason in reasons() {
        let mut h = Harness::setup();
        let move_call = admin(AdminRequest::Move {
            transfer_id: transfer(1),
            project: project(),
            to_installation: machine(2),
            reason: "move".into(),
        });
        assert!(matches!(
            h.call("registry", move_call.clone()),
            WireResult::Registry(RegistryResult::Admin(AdminResult::Pending { .. }))
        ));
        let release = WireCall::Registry(RegistryCall::Release {
            installation: machine(1),
            project: project(),
            epoch: epoch(1),
            transfer: transfer(1),
            outcome: ReleaseOutcome::Final(position(1, 0)),
        });
        let (_, effects) = h.begin("registry", release.clone());
        let before = records(&mut h, "_registry_transfers");
        let leases = records(&mut h, "_registry_leases");
        let effects = fail_copy(&mut h, effects, reason.clone());
        assert!(matches!(effects.as_slice(), [WireEffect::Return {
            result: WireResult::Registry(RegistryResult::Release(SessionAnswer::Unavailable {
                subject: BlockedSubject::Transfer { transfer: target }, reason: actual,
            })), ..
        }] if target == &transfer(1) && actual == &reason));
        assert_eq!(records(&mut h, "_registry_transfers"), before);
        assert_eq!(records(&mut h, "_registry_leases"), leases);
        h.restart_registry();
        for _ in 0..2 {
            assert_eq!(
                h.call("registry", release.clone()),
                WireResult::Registry(RegistryResult::Release(SessionAnswer::Released {
                    final_position: position(1, 0)
                }))
            );
        }
        assert!(
            matches!(h.call("registry", move_call), WireResult::Registry(RegistryResult::Admin(AdminResult::Granted { new_epoch, .. })) if new_epoch == epoch(2))
        );
    }
}

#[test]
fn unavailable_release_schema_probe_keeps_its_chosen_state_until_real_schema_refusal() {
    let mut h = Harness::setup();
    h.call(
        "registry",
        admin(AdminRequest::Move {
            transfer_id: transfer(1),
            project: project(),
            to_installation: machine(2),
            reason: "move".into(),
        }),
    );
    h.enroll(2); // Re-enrollment has no schema advertisement.
    let release = WireCall::Registry(RegistryCall::Release {
        installation: machine(1),
        project: project(),
        epoch: epoch(1),
        transfer: transfer(1),
        outcome: ReleaseOutcome::Final(position(1, 0)),
    });
    let (_, effects) = h.begin("registry", release.clone());
    assert!(effects.iter().any(|effect| matches!(
        effect,
        WireEffect::Dispatch {
            call: WireCall::Copy(CopyCall::CopyStatus { .. }),
            ..
        }
    )));
    let before = records(&mut h, "_registry_transfers");
    let effects = fail_copy(&mut h, effects, CopyUnavailableReason::StorageFull);
    assert!(matches!(effects.as_slice(), [WireEffect::Return {
        result: WireResult::Registry(RegistryResult::Release(SessionAnswer::Unavailable {
            subject: BlockedSubject::Transfer { transfer: target }, ..
        })), ..
    }] if target == &transfer(1)));
    assert_eq!(records(&mut h, "_registry_transfers"), before);
    h.restart_registry();
    assert!(matches!(
        h.call("registry", release),
        WireResult::Registry(RegistryResult::Release(SessionAnswer::ReleaseRefused {
            reason: ReleaseRefusal::SchemaTooNew {
                supported: None,
                ..
            },
            ..
        }))
    ));
}

fn replay_snapshot(h: &mut Harness) -> std::collections::BTreeMap<&'static str, Vec<String>> {
    [
        ("lease", "registry", "SELECT record FROM _registry_leases ORDER BY project"),
        ("transfer", "registry", "SELECT record FROM _registry_transfers ORDER BY transfer_id"),
        ("history", "registry", "SELECT json_array(project,epoch,installation,cutoff) FROM _registry_history ORDER BY project,epoch"),
        ("copy", "copy", "SELECT json_array(project,holder,fence_epoch,start_epoch,start_commit,acked_epoch,acked_commit,schema_version,schema_fingerprint,archived,last_piece_at) FROM _copy_state"),
        ("receipt", "copy", "SELECT json_array(transfer_id,old_epoch,new_epoch,start_epoch,start_commit,old_holder) FROM _copy_transfers ORDER BY transfer_id"),
    ].into_iter().map(|(label, actor, sql)| {
        let mut query = h.store(actor).connection.prepare(sql).expect("domain snapshot");
        let rows = query.query_map([], |row| row.get::<_, String>(0)).expect("snapshot query")
            .collect::<Result<Vec<_>, _>>().expect("snapshot rows");
        (label, rows)
    }).collect()
}

#[test]
fn later_clock_grant_replay_keeps_expiry_history_and_fence_receipt_without_new_effects() {
    for committed in [false, true] {
        let mut h = Harness::setup();
        let request = h.takeover_call(91);
        let (_, effects) = h.begin("registry", request.clone());
        if committed {
            let fence = effects
                .iter()
                .find_map(|effect| match effect {
                    WireEffect::Dispatch {
                        call: call @ WireCall::Copy(_),
                        ..
                    } => Some(call.clone()),
                    _ => None,
                })
                .expect("copy fence");
            assert!(
                matches!(h.call("copy", fence), WireResult::Copy(CopyResult::Fence(FenceResult::Fenced { new_epoch, .. })) if new_epoch == epoch(2))
            );
        }
        let before = replay_snapshot(&mut h);
        let unavailable = fail_copy(&mut h, effects, CopyUnavailableReason::StorageFull);
        assert!(matches!(
            unavailable.as_slice(),
            [WireEffect::Return {
                result: WireResult::Registry(RegistryResult::Admin(
                    AdminResult::Unavailable { .. }
                )),
                ..
            }]
        ));
        assert_eq!(replay_snapshot(&mut h), before);
        h.restart_registry();
        h.now = at(10);
        let (input, effects) = h.begin("registry", request.clone());
        let (granted, trace) = h.finish("registry", &input, effects);
        assert!(
            matches!(granted, WireResult::Registry(RegistryResult::Admin(AdminResult::Granted { new_epoch, start })) if new_epoch == epoch(2) && start == position(1, 0))
        );
        assert_eq!(
            trace
                .iter()
                .filter(|effect| matches!(
                    effect,
                    WireEffect::Notify {
                        kind: NoticeKind::LeaseGranted { .. },
                        ..
                    }
                ))
                .count(),
            1
        );
        let stable = replay_snapshot(&mut h);
        assert_eq!(stable["transfer"].len(), 1);
        assert_eq!(stable["history"].len(), 2);
        assert_eq!(stable["receipt"].len(), 1);
        let lease: RegistryLease =
            serde_json::from_str(&stable["lease"][0]).expect("granted lease");
        assert_eq!(lease.expires_at, at(250).0);
        h.now = at(20);
        let (input, effects) = h.begin("registry", request);
        let (replayed, trace) = h.finish("registry", &input, effects);
        assert_eq!(replayed, granted);
        assert_eq!(
            replay_snapshot(&mut h),
            stable,
            "later replay cannot renew or re-grant"
        );
        assert!(
            trace
                .iter()
                .all(|effect| matches!(effect, WireEffect::Return { .. })),
            "stored replay has no dispatch, hint or alarm"
        );
    }
}
