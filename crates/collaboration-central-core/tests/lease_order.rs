#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;

use collaboration_central_core::*;
use registry_support::{identity, start};

fn fleet_call(
    store: &mut registry_support::sql_support::SqlTxFake,
    call: serde_json::Value,
) -> FleetResult {
    let call: FleetCall = serde_json::from_value(call).expect("reviewed Fleet call");
    let effects = store
        .run(
            WireInput::Call {
                from: ObjectAddress::Registry,
                call_id: ObjectCallId::new(identity(980)).expect("call"),
                call: Box::new(WireCall::Fleet(call)),
            },
            start(),
        )
        .expect("Fleet transaction");
    effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Return {
                result: WireResult::Fleet(result),
                ..
            } => Some(result),
            _ => None,
        })
        .expect("Fleet result")
}

#[test]
fn fleet_incarnation_fence_advances_above_both_durable_and_offered_floors() {
    let mut store = registry_support::sql_support::SqlTxFake::blank();
    // Discarding an earlier answer does not reuse its allocated incarnation.
    // A restored local floor may be lower than Fleet's retained witness.
    for (at_least, expected) in [(0, 1), (0, 2), (9, 10), (3, 11)] {
        let result = fleet_call(
            &mut store,
            serde_json::json!({
                "kind":"advanceLeaseIncarnation",
                "value":{"at_least":at_least}
            }),
        );
        assert_eq!(
            serde_json::to_value(result).expect("result"),
            serde_json::json!({"kind":"leaseIncarnationAdvanced", "value":expected})
        );
    }
}

#[test]
fn lease_versions_order_epoch_then_incarnation_then_change_and_reject_zero_components() {
    use registry_support::epoch;
    let version = |epoch_number, incarnation, change| {
        LeaseVersion::new(
            epoch(epoch_number),
            RegistryIncarnation::new(incarnation).expect("incarnation"),
            change,
        )
        .expect("version")
    };
    assert!(version(2, 1, 1) > version(1, u64::MAX, u64::MAX));
    assert!(version(2, 9, 1) > version(2, 8, u64::MAX));
    assert!(version(2, 9, 11) > version(2, 9, 10));
    let expected = serde_json::json!({"epoch":2,"incarnation":9,"change":11});
    assert_eq!(
        serde_json::to_value(version(2, 9, 11)).expect("encode"),
        expected
    );
    assert_eq!(
        serde_json::from_value::<LeaseVersion>(expected).expect("decode"),
        version(2, 9, 11)
    );
    for invalid in [
        serde_json::json!({"epoch":0,"incarnation":9,"change":11}),
        serde_json::json!({"epoch":2,"incarnation":0,"change":11}),
        serde_json::json!({"epoch":2,"incarnation":9,"change":0}),
    ] {
        assert!(serde_json::from_value::<LeaseVersion>(invalid).is_err());
    }
}

#[test]
fn a_restarted_registry_decides_renewal_but_gates_publication_until_fleet_allocation() {
    use registry_support::*;
    let mut harness = Harness::setup();
    harness.restart_registry();
    harness.now = at(1);
    let (_, effects) = harness.begin(
        "registry",
        WireCall::Registry(RegistryCall::Renew {
            installation: machine(1),
            attempt: request(970),
            project: project(),
            epoch: epoch(1),
        }),
    );
    assert!(effects.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Renew(collaboration_replication_protocol::SessionAnswer::Renewed { replayed: false, expires_at, .. })), .. } if *expires_at == at(241).0)), "lease decision is unchanged while Fleet is unavailable");
    assert!(
        !effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch {
                call: WireCall::Fleet(FleetCall::DirectoryUpdate(_)),
                ..
            }
        )),
        "a fresh registry run cannot publish with an unallocated incarnation"
    );
    assert!(
        effects.iter().any(|effect| matches!(
            effect,
            WireEffect::Dispatch {
                call: WireCall::Fleet(FleetCall::AdvanceLeaseIncarnation { .. }),
                ..
            }
        )),
        "the only publication prerequisite is the real Fleet allocator"
    );
}
