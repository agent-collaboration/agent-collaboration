//! The selected fault remains armed when another call uses the same destination.

use super::*;
use crate::{DispatchCallSelector, DispatchFaultObservation, DispatchKey};
use serde_json::json;
use std::time::Duration;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(2);
const SELECTED_KINDS: [&str; 4] = [
    "central.mail-submit",
    "central.installation-peer",
    "central.forward-route",
    "central.copy-session-write",
];

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "The exact Fleet submit must be held while another peer or item progresses through the same local seam."
)]
async fn fleet_submit_selector_holds_only_its_authenticated_peer_and_item() -> TestResult {
    let peer = "019f0000-0000-7000-8000-000000000910";
    let item = "019f0000-0000-7000-8000-000000000911";
    let submission = |peer: &str, item: &str| {
        json!({
            "kind":"central.mail-submit",
            "identity":{"operation":"019f0000-0000-7000-8000-000000000912","step":"fleet-submit"},
            "request":{"session":{"peer":peer,"sessionId":"019f0000-0000-7000-8000-000000000913"},"item":{"kind":"item","value":{"id":item}}}
        })
    };
    let expected = serde_json::to_vec(&submission(peer, item))?;
    let other_item = serde_json::to_vec(&submission(peer, "019f0000-0000-7000-8000-000000000914"))?;
    let other_peer = serde_json::to_vec(&submission("019f0000-0000-7000-8000-000000000915", item))?;
    let faults = RuntimeFaults::default();
    let destination = crate::legacy_fleet_address();
    let key = DispatchKey::new(
        destination.clone(),
        DispatchCallSelector::FleetMailSubmitFor {
            peer: peer.to_owned().try_into()?,
            item: item.to_owned().try_into()?,
        },
    );
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    for unrelated in [&other_item, &other_peer] {
        tokio::time::timeout(BOUND, faults.before_dispatch(&destination, unrelated)).await??;
        assert!(matches!(
            faults.observe()?.dispatch(&key),
            DispatchFaultObservation::Armed { .. }
        ));
    }
    let selected = faults.before_dispatch(&destination, &expected);
    tokio::pin!(selected);
    assert!(
        futures_util::poll!(&mut selected).is_pending(),
        "current Fleet submission bypassed its exact selector"
    );
    assert!(
        matches!(faults.control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }).await?.dispatch(&key),DispatchFaultObservation::Held {request,..} if request==expected)
    );
    for unrelated in [&other_item, &other_peer] {
        tokio::time::timeout(BOUND, faults.before_dispatch(&destination, unrelated)).await??;
    }
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, selected).await??;
    faults
        .control(RuntimeControl::DropNextSelectedObjectReply {
            destination: destination.clone(),
            call: DispatchCallSelector::FleetMailSubmitFor {
                peer: peer.to_owned().try_into()?,
                item: item.to_owned().try_into()?,
            },
        })
        .await?;
    for unrelated in [&other_item, &other_peer] {
        assert!(!faults.drop_successful_object_reply(&destination, unrelated)?);
    }
    assert!(faults.drop_successful_object_reply(&destination, &expected)?);
    assert!(!faults.drop_successful_object_reply(&destination, &expected)?);
    Ok(())
}

fn other_object() -> Result<ObjectAddress, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ObjectAddress::Copy(
        "019f0000-0000-7000-8000-000000000899"
            .to_owned()
            .try_into()?,
    ))
}

fn invocation(kind: &str) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "kind": kind,
        "identity": {"operation":"019f0000-0000-7000-8000-000000000806","step":"selected"},
        "request": {"text":"central.mail-submit central.installation-peer central.forward-route central.copy-session-write"},
    }))
}

async fn selected_hold(
    call: DispatchCallSelector,
    kind: &str,
    destination: ObjectAddress,
) -> TestResult {
    let key = DispatchKey::new(destination.clone(), call);
    let faults = Arc::new(RuntimeFaults::default());
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    let unrelated = invocation("central.sender-mail")?;
    let status = serde_json::to_vec(&json!({
        "kind":"central.copy-session-read", "request":{"kind":"status"}
    }))?;
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &unrelated)).await??;
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &status)).await??;
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, b"malformed")).await??;
    for other in SELECTED_KINDS.into_iter().filter(|other| *other != kind) {
        tokio::time::timeout(
            BOUND,
            faults.before_dispatch(&destination, &invocation(other)?),
        )
        .await??;
    }
    assert!(matches!(
        faults.observe()?.dispatch(&key),
        DispatchFaultObservation::Armed { .. }
    ));
    let expected = invocation(kind)?;
    tokio::time::timeout(BOUND, faults.before_dispatch(&other_object()?, &expected)).await??;

    let caller = faults.clone();
    let request = expected.clone();
    let held_destination = destination.clone();
    let forwarding =
        tokio::spawn(async move { caller.before_dispatch(&held_destination, &request).await });
    let held = tokio::time::timeout(
        BOUND,
        faults.control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }),
    )
    .await??;
    assert!(
        matches!(held.dispatch(&key), DispatchFaultObservation::Held { destination: actual, request }
        if actual == destination && request == expected)
    );
    assert!(!forwarding.is_finished());
    // A selected hold still permits other traffic: F2's parallel slots retain their semantics.
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &unrelated)).await??;
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &status)).await??;
    // Even a later matching call passes while the first call is held.
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &expected)).await??;
    assert!(!forwarding.is_finished());
    faults
        .control(RuntimeControl::ResumeDispatchAndHoldNext {
            key: key.clone(),
            next: key.clone(),
        })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await???;
    tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &unrelated)).await??;
    assert!(matches!(
        faults.observe()?.dispatch(&key),
        DispatchFaultObservation::Armed { .. }
    ));
    let caller = faults.clone();
    let request = expected.clone();
    let forwarding =
        tokio::spawn(async move { caller.before_dispatch(&destination, &request).await });
    let retry = tokio::time::timeout(
        BOUND,
        faults.control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }),
    )
    .await??;
    assert!(
        matches!(retry.dispatch(&key), DispatchFaultObservation::Held { request, .. } if request == expected)
    );
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await???;
    assert!(matches!(
        faults.observe()?.dispatch(&key),
        DispatchFaultObservation::Idle
    ));
    Ok(())
}

#[tokio::test]
async fn fleet_submit_hold_ignores_sender_requests_and_preserves_the_selected_bytes() -> TestResult
{
    for (call, kind) in [
        (DispatchCallSelector::FleetMailSubmit, "central.mail-submit"),
        (DispatchCallSelector::ForwardRoute, "central.forward-route"),
    ] {
        selected_hold(call, kind, crate::legacy_fleet_address()).await?;
    }
    Ok(())
}

#[tokio::test]
async fn installation_peer_hold_ignores_other_outer_kinds_and_releases_only_the_selected_call()
-> TestResult {
    selected_hold(
        DispatchCallSelector::InstallationPeer,
        "central.installation-peer",
        crate::legacy_fleet_address(),
    )
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn copy_write_hold_ignores_status_and_unknown_calls_without_freezing_later_writes()
-> TestResult {
    assert_eq!(
        serde_json::to_value(DispatchCallSelector::CopyWrite)?,
        json!("copyWrite")
    );
    selected_hold(
        DispatchCallSelector::CopyWrite,
        "central.copy-session-write",
        ObjectAddress::Copy(
            "019f0000-0000-7000-8000-000000000807"
                .to_owned()
                .try_into()?,
        ),
    )
    .await
}

async fn selected_drop(call: DispatchCallSelector, kind: &str) -> TestResult {
    let faults = RuntimeFaults::default();
    let destination = crate::legacy_fleet_address();
    faults
        .control(RuntimeControl::DropNextSelectedObjectReply {
            destination: destination.clone(),
            call,
        })
        .await?;
    let unrelated = invocation("central.mailbox-request")?;
    assert!(!faults.drop_successful_object_reply(&destination, &unrelated)?);
    assert!(!faults.drop_successful_object_reply(&destination, b"malformed")?);
    for other in SELECTED_KINDS.into_iter().filter(|other| *other != kind) {
        assert!(!faults.drop_successful_object_reply(&destination, &invocation(other)?)?);
    }
    assert!(matches!(
        faults.observe()?.object_reply,
        ObjectReplyFaultObservation::Armed { .. }
    ));
    let expected = invocation(kind)?;
    assert!(!faults.drop_successful_object_reply(&other_object()?, &expected)?);
    assert!(faults.drop_successful_object_reply(&destination, &expected)?);
    let observed = tokio::time::timeout(
        BOUND,
        faults.control(RuntimeControl::AwaitDroppedObjectReply),
    )
    .await??;
    assert!(
        matches!(observed.object_reply, ObjectReplyFaultObservation::Dropped { destination: actual, request }
            if actual == destination && request == expected)
    );
    assert!(!faults.drop_successful_object_reply(&destination, &expected)?);
    Ok(())
}

#[tokio::test]
async fn fleet_submit_reply_drop_ignores_background_requests_and_drops_the_selected_reply_once()
-> TestResult {
    for (call, kind) in [
        (DispatchCallSelector::FleetMailSubmit, "central.mail-submit"),
        (DispatchCallSelector::ForwardRoute, "central.forward-route"),
    ] {
        selected_drop(call, kind).await?;
    }
    Ok(())
}

#[tokio::test]
async fn installation_peer_reply_drop_ignores_other_kinds_and_retains_the_original_identity()
-> TestResult {
    selected_drop(
        DispatchCallSelector::InstallationPeer,
        "central.installation-peer",
    )
    .await
}

#[tokio::test]
async fn copy_write_reply_drop_selects_only_the_outer_write_kind_once() -> TestResult {
    selected_drop(
        DispatchCallSelector::CopyWrite,
        "central.copy-session-write",
    )
    .await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn existing_any_controls_still_accept_opaque_calls_and_resume_before_the_next_hold()
-> TestResult {
    let faults = Arc::new(RuntimeFaults::default());
    let destination = crate::legacy_fleet_address();
    let key = DispatchKey::any(destination.clone());
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    let first = faults.clone();
    let held_destination = destination.clone();
    let forwarding =
        tokio::spawn(async move { first.before_dispatch(&held_destination, b"opaque").await });
    tokio::time::timeout(
        BOUND,
        faults.control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }),
    )
    .await??;
    faults
        .control(RuntimeControl::ResumeDispatchAndHoldNext {
            key: key.clone(),
            next: key.clone(),
        })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await???;
    assert!(matches!(
        faults.observe()?.dispatch(&key),
        DispatchFaultObservation::Armed { .. }
    ));
    let second = faults.clone();
    let held_destination = destination.clone();
    let forwarding = tokio::spawn(async move {
        second
            .before_dispatch(&held_destination, b"second opaque")
            .await
    });
    let held = tokio::time::timeout(
        BOUND,
        faults.control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }),
    )
    .await??;
    assert!(
        matches!(held.dispatch(&key), DispatchFaultObservation::Held { request, .. } if request == b"second opaque")
    );
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await???;
    faults
        .control(RuntimeControl::DropNextObjectReply {
            destination: destination.clone(),
        })
        .await?;
    assert!(faults.drop_successful_object_reply(&destination, b"opaque reply")?);
    Ok(())
}

fn fleet_submit_identity(peer: &str, item: &str, operation: &str) -> serde_json::Value {
    json!({
        "kind": "central.mail-submit",
        "identity": {"operation": operation, "step": "selected-item"},
        "request": {"session": {"peer": peer}, "item": {"kind": "item", "value": {"id": item}}},
    })
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
async fn fleet_item_selection_ignores_other_peers_items_cancels_and_queries() -> TestResult {
    let peer = "019f0000-0000-7000-8000-000000000810";
    let item = "019f0000-0000-7000-8000-000000000811";
    let operation = "019f0000-0000-7000-8000-000000000812";
    let selector = DispatchCallSelector::FleetMailSubmitFor {
        peer: peer.to_owned().try_into()?,
        item: item.to_owned().try_into()?,
    };
    assert_eq!(
        serde_json::to_value(selector)?,
        json!({"fleetMailSubmitFor": {"peer": peer, "item": item}})
    );
    let selector = serde_json::from_value(serde_json::to_value(selector)?)?;
    let faults = RuntimeFaults::default();
    let destination = crate::legacy_fleet_address();
    let key = DispatchKey::new(destination.clone(), selector);
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    let expected = fleet_submit_identity(peer, item, operation);
    let mut wrong_peer = expected.clone();
    wrong_peer["request"]["session"]["peer"] = json!("019f0000-0000-7000-8000-000000000813");
    let mut wrong_item = expected.clone();
    wrong_item["request"]["item"]["value"]["id"] = json!("019f0000-0000-7000-8000-000000000814");
    let mut cancel = wrong_item.clone();
    cancel["request"]["item"]["kind"] = json!("cancel");
    let mut query = expected.clone();
    query["kind"] = json!("central.mailbox-request");
    let mut invalid = expected.clone();
    invalid["request"]["item"]["value"]["id"] = json!("invalid");
    let mut missing = expected.clone();
    missing["request"] = json!({"peer": peer});
    let mut retired_mailbox = expected.clone();
    retired_mailbox["kind"] = json!("central.mailbox-submit");
    for unrelated in [
        wrong_peer,
        wrong_item,
        cancel,
        query,
        invalid,
        missing,
        retired_mailbox,
    ] {
        tokio::time::timeout(
            BOUND,
            faults.before_dispatch(&destination, &serde_json::to_vec(&unrelated)?),
        )
        .await??;
        assert!(matches!(
            faults.observe()?.dispatch(&key),
            DispatchFaultObservation::Armed { .. }
        ));
    }
    let expected = serde_json::to_vec(&expected)?;
    let forwarding = faults.before_dispatch(&destination, &expected);
    tokio::pin!(forwarding);
    assert!(futures_util::poll!(&mut forwarding).is_pending());
    let held = faults
        .control(RuntimeControl::AwaitHeldDispatch { key: key.clone() })
        .await?;
    assert!(
        matches!(held.dispatch(&key), DispatchFaultObservation::Held { request, .. }
        if request == expected)
    );
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await??;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
async fn held_reply_binding_rejects_unheld_states_and_preserves_an_armed_fault() -> TestResult {
    let faults = RuntimeFaults::default();
    let destination = crate::legacy_fleet_address();
    let key = DispatchKey::any(destination.clone());
    assert!(matches!(
        faults
            .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
            .await,
        Err(RuntimeControlFailure::NotHeld)
    ));
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    assert!(matches!(
        faults
            .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
            .await,
        Err(RuntimeControlFailure::NotHeld)
    ));
    assert!(matches!(
        faults.observe()?.object_reply,
        ObjectReplyFaultObservation::Idle
    ));
    let forwarding = faults.before_dispatch(&destination, b"held request");
    tokio::pin!(forwarding);
    assert!(futures_util::poll!(&mut forwarding).is_pending());
    faults
        .control(RuntimeControl::DropNextObjectReply {
            destination: other_object()?,
        })
        .await?;
    assert!(matches!(
        faults
            .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
            .await,
        Err(RuntimeControlFailure::AlreadyArmed)
    ));
    assert!(!faults.drop_successful_object_reply(&destination, b"held request")?);
    assert!(faults.drop_successful_object_reply(&other_object()?, b"original fault")?);
    // As with the existing reply controls, Dropped can be replaced by a new arm.
    faults
        .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
        .await?;
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await??;
    assert!(faults.drop_successful_object_reply(&destination, b"held request")?);
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions fail the test while fixture errors propagate."
)]
async fn held_reply_binding_ignores_a_bypassing_submit_and_same_item_with_new_call_bytes()
-> TestResult {
    let faults = RuntimeFaults::default();
    let destination = crate::legacy_fleet_address();
    let key = DispatchKey::any(destination.clone());
    let expected = fleet_submit_identity(
        "019f0000-0000-7000-8000-000000000810",
        "019f0000-0000-7000-8000-000000000811",
        "019f0000-0000-7000-8000-000000000812",
    );
    let mut cancel = expected.clone();
    cancel["request"]["item"]["kind"] = json!("cancel");
    cancel["request"]["item"]["value"]["id"] = json!("019f0000-0000-7000-8000-000000000813");
    let mut new_call = expected.clone();
    new_call["identity"]["operation"] = json!("019f0000-0000-7000-8000-000000000814");
    let expected = serde_json::to_vec(&expected)?;
    faults
        .control(RuntimeControl::HoldNextDispatch { key: key.clone() })
        .await?;
    let forwarding = faults.before_dispatch(&destination, &expected);
    tokio::pin!(forwarding);
    assert!(futures_util::poll!(&mut forwarding).is_pending());
    faults
        .control(RuntimeControl::DropHeldDispatchReply { key: key.clone() })
        .await?;
    for unrelated in [serde_json::to_vec(&cancel)?, serde_json::to_vec(&new_call)?] {
        tokio::time::timeout(BOUND, faults.before_dispatch(&destination, &unrelated)).await??;
        assert!(!faults.drop_successful_object_reply(&destination, &unrelated)?);
        assert!(matches!(
            faults.observe()?.object_reply,
            ObjectReplyFaultObservation::Armed { .. }
        ));
    }
    assert!(!faults.drop_successful_object_reply(&other_object()?, &expected)?);
    faults
        .control(RuntimeControl::ResumeDispatch { key: key.clone() })
        .await?;
    tokio::time::timeout(BOUND, forwarding).await??;
    assert!(faults.drop_successful_object_reply(&destination, &expected)?);
    let dropped = faults
        .control(RuntimeControl::AwaitDroppedObjectReply)
        .await?;
    assert!(
        matches!(dropped.object_reply, ObjectReplyFaultObservation::Dropped { request, .. }
        if request == expected)
    );
    assert!(!faults.drop_successful_object_reply(&destination, &expected)?);
    Ok(())
}
