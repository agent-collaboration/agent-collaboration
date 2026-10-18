#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;

fn single_fence_dispatch(
    effects: &[WireEffect],
) -> (ObjectCallId, WireCall, Option<DispatchCarry>) {
    let dispatches: Vec<_> = effects
        .iter()
        .filter_map(|e| match e {
            WireEffect::Dispatch {
                call_id,
                to: ObjectAddress::Copy(_),
                call: call @ WireCall::Copy(CopyCall::Fence { .. }),
                carry,
            } => Some((call_id.clone(), call.clone(), carry.clone())),
            WireEffect::Dispatch { .. } => panic!("unexpected dispatch {e:?}"),
            _ => None,
        })
        .collect();
    assert_eq!(dispatches.len(), 1, "one fence attempt in {effects:?}");
    dispatches.into_iter().next().expect("fence dispatch")
}

/// Fences the real copy ahead of the registry, as a restored registry or a
/// lost transfer would leave it.
fn fence_copy_out_of_band(h: &mut Harness, t: u64, from: u64, to: u64) {
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::Fence {
                holder: machine(1),
                transfer_id: transfer(t),
                new_epoch: epoch(to),
                expect: FenceExpectation {
                    epoch: epoch(from),
                    acked: position(1, 0),
                },
                target_schema_max: 10,
            }),
        ),
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(from),
            new_epoch: epoch(to),
            start: position(1, 0),
        }))
    );
}

fn stored_value(h: &mut Harness, sql: &str) -> String {
    h.store("registry")
        .connection
        .query_row(sql, [project().as_str()], |r| r.get(0))
        .expect("registry row")
}

#[test]
fn floor_repair_refused_again_is_unproven_without_grant_notice_lease_or_floor_change() {
    // Arrange: the takeover selects 2 from the preview's copy fence 1.
    let mut h = Harness::setup();
    let takeover = h.takeover_call(1);
    let lease_sql = "SELECT record FROM _registry_leases WHERE project=?";
    let floor_sql = "SELECT epoch FROM _registry_epoch_floor WHERE project=?";
    let lease_before = stored_value(&mut h, lease_sql);
    let floor_before = stored_value(&mut h, floor_sql);
    let (input, effects) = h.begin("registry", takeover);
    let mut trace = effects.clone();
    let (call_id, initial, carry) = single_fence_dispatch(&effects);
    assert!(
        matches!(&initial, WireCall::Copy(CopyCall::Fence { new_epoch, .. }) if *new_epoch == epoch(2)),
        "{initial:?}"
    );

    // Act: the copy moves past each attempt before that attempt reaches it.
    fence_copy_out_of_band(&mut h, 9, 1, 5);
    let refused = h.call("copy", initial);
    assert_eq!(
        refused,
        WireResult::Copy(CopyResult::Fence(FenceResult::FenceNotAdvanced {
            current_epoch: epoch(5)
        }))
    );
    let effects = h.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            carry,
            call_id,
            result: Box::new(refused),
        },
    );
    trace.extend(effects.clone());
    let (call_id, repair, carry) = single_fence_dispatch(&effects);
    assert!(
        matches!(&repair, WireCall::Copy(CopyCall::Fence { new_epoch, expect, .. })
            if *new_epoch == epoch(6) && expect.epoch == epoch(5)),
        "floor repair selects above the copy's answer: {repair:?}"
    );
    fence_copy_out_of_band(&mut h, 10, 5, 7);
    let refused_again = h.call("copy", repair);
    assert_eq!(
        refused_again,
        WireResult::Copy(CopyResult::Fence(FenceResult::FenceNotAdvanced {
            current_epoch: epoch(7)
        }))
    );
    let effects = h.run(
        "registry",
        WireInput::CallResult {
            source: CallerKind::Registry,
            carry,
            call_id,
            result: Box::new(refused_again),
        },
    );
    trace.extend(effects.clone());

    // Assert
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, WireEffect::Dispatch { .. })),
        "only one floor-repair attempt: {effects:?}"
    );
    validate_call_returns(&input, &trace).expect("one Return across the takeover");
    assert!(trace.iter().any(|e| matches!(
        e,
        WireEffect::Return {
            result: WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
                AdminRefusal::RegistryEpochUnproven
            ))),
            ..
        }
    )));
    assert!(
        !trace.iter().any(|e| matches!(
            e,
            WireEffect::Notify {
                kind: NoticeKind::LeaseGranted { .. } | NoticeKind::LeaseSuperseded { .. },
                ..
            }
        )),
        "no lease notice: {trace:?}"
    );
    assert_eq!(stored_value(&mut h, lease_sql), lease_before);
    assert_eq!(stored_value(&mut h, floor_sql), floor_before);
    assert_eq!(floor_before, "1");
    assert!(
        matches!(h.renew(1, 1, 1), SessionAnswer::Renewed { current, transfer_in_progress: None, .. }
            if current.epoch == epoch(1) && current.holder == machine(1))
    );
}
