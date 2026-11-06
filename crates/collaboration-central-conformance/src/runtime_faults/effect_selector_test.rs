//! Post-commit Fleet effects must not capture their enclosing request or read.
use super::RuntimeFaults;
use crate::{DispatchCallSelector, DispatchFaultObservation, DispatchKey, RuntimeControl};
use collaboration_central_core::ObjectAddress;
use serde_json::json;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(2);

fn invocation(kind: &str) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&json!({"kind":kind,
        "identity":{"operation":"019f0000-0000-7000-8000-000000008731","step":"original_effect"},
        "request":{"text":"central.machine-notice fleet-sync.claim-settlement"}
    }))
}

async fn selected_effect(call: DispatchCallSelector, kind: &str, wire_name: &str) -> TestResult {
    assert_eq!(serde_json::to_value(call)?, json!(wire_name));
    let key = DispatchKey::new(ObjectAddress::Fleet, call);
    let faults = RuntimeFaults::default();
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    let expected = invocation(kind)?;
    let dispatch = faults.before_dispatch(&ObjectAddress::Fleet, &expected);
    tokio::pin!(dispatch);
    assert!(
        futures_util::poll!(&mut dispatch).is_pending(),
        "exact effect bypassed its barrier"
    );
    assert!(
        matches!(faults.control(RuntimeControl::AwaitHeldDispatch {key:key.clone()}).await?.dispatch(&key),
        DispatchFaultObservation::Held {request,..} if request==expected)
    );
    faults
        .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
        .await?;
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, dispatch).await??;
    assert!(faults.drop_successful_object_reply(&ObjectAddress::Fleet, &expected)?);
    assert!(!faults.drop_successful_object_reply(&ObjectAddress::Fleet, &expected)?);
    assert!(faults.observe()?.dispatches.is_empty());
    Ok(())
}

async fn unrelated_effects(call: DispatchCallSelector, kind: &str, other: &str) -> TestResult {
    let key = DispatchKey::new(ObjectAddress::Fleet, call);
    let faults = RuntimeFaults::default();
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    for unrelated in [
        "central.fleet-metadata",
        "central.session-lease",
        "central.fleet-read",
        other,
    ] {
        tokio::time::timeout(
            BOUND,
            faults.before_dispatch(&ObjectAddress::Fleet, &invocation(unrelated)?),
        )
        .await??;
        assert!(matches!(
            faults.observe()?.dispatch(&key),
            DispatchFaultObservation::Armed { .. }
        ));
    }
    tokio::time::timeout(
        BOUND,
        faults.before_dispatch(&ObjectAddress::Fleet, b"malformed"),
    )
    .await??;
    let other_destination = ObjectAddress::Copy(
        "019f0000-0000-7000-8000-000000008732"
            .to_owned()
            .try_into()?,
    );
    tokio::time::timeout(
        BOUND,
        faults.before_dispatch(&other_destination, &invocation(kind)?),
    )
    .await??;
    assert!(matches!(
        faults.observe()?.dispatch(&key),
        DispatchFaultObservation::Armed { .. }
    ));
    Ok(())
}

#[tokio::test]
async fn machine_notice_selector_preserves_the_held_effect_and_drops_only_its_original_reply()
-> TestResult {
    selected_effect(
        DispatchCallSelector::FleetMachineNotice,
        "central.machine-notice",
        "fleetMachineNotice",
    )
    .await
}

#[tokio::test]
async fn machine_notice_selector_ignores_enclosing_requests_and_settlement_calls() -> TestResult {
    unrelated_effects(
        DispatchCallSelector::FleetMachineNotice,
        "central.machine-notice",
        "fleet-sync.claim-settlement",
    )
    .await
}

#[tokio::test]
async fn claim_settlement_selector_preserves_the_held_effect_and_drops_only_its_original_reply()
-> TestResult {
    selected_effect(
        DispatchCallSelector::FleetClaimSettlement,
        "fleet-sync.claim-settlement",
        "fleetClaimSettlement",
    )
    .await
}

#[tokio::test]
async fn claim_settlement_selector_ignores_enclosing_requests_and_machine_notices() -> TestResult {
    unrelated_effects(
        DispatchCallSelector::FleetClaimSettlement,
        "fleet-sync.claim-settlement",
        "central.machine-notice",
    )
    .await
}
