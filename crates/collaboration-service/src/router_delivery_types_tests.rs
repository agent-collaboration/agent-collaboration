//! The shared marker and Router marker encode the same content and digests in both directions.
use super::*;
use collaboration_protocol::{
    DeliveryNextAction, DeliveryOutcome, DeliveryPeerClaim, DeliveryReceipt, DeliveryRejection,
    DeliveryRejectionReason, SessionReachability,
};
use fleet_sync_protocol::{CancelBody, DeliveryCancel, DeliveryItem, ItemId, PeerId, Recipient};
use serde_json::json;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn shared_and_router_items_and_cancels_have_identical_wire_bytes_and_digests() -> TestResult {
    let id = ItemId::new(uuid::Uuid::now_v7())?;
    let sender = serde_json::from_value(json!({
        "kind":"humanUser", "installation":"00000000-0000-4000-8000-000000000002"
    }))?;
    let recipient = Recipient {
        peer: PeerId::new(uuid::Uuid::now_v7())?,
        address: serde_json::from_value(json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
            "sessionId":"wire-identity-recipient"
        }))?,
    };
    let shared = DeliveryItem::<BoardDeliveryTypes>::new(
        id,
        sender,
        recipient,
        None,
        serde_json::from_value(
            json!({"kind":"message","text":"identical content","delivery":"queue","guard":null,"senderDisplayName":null,"replyTo":null}),
        )?,
    )?;
    let wire = serde_json::to_vec(&shared)?;
    let router: DeliveryItem<RouterDeliveryTypes> = serde_json::from_slice(&wire)?;
    let reverse = serde_json::to_vec(&router)?;
    if wire != reverse || shared.digest() != router.digest() {
        return Err("Router item changed the shared wire bytes or digest".into());
    }
    let shared_again: DeliveryItem<BoardDeliveryTypes> = serde_json::from_slice(&reverse)?;
    if shared.digest() != shared_again.digest() {
        return Err("reverse item decoding changed the shared digest".into());
    }
    let shared_cancel = DeliveryCancel::<BoardDeliveryTypes>::new(
        ItemId::new(uuid::Uuid::now_v7())?,
        shared.sender().clone(),
        shared.recipient().clone(),
        None,
        CancelBody { target: id },
    )?;
    let wire = serde_json::to_vec(&shared_cancel)?;
    let router_cancel: DeliveryCancel<RouterDeliveryTypes> = serde_json::from_slice(&wire)?;
    let reverse = serde_json::to_vec(&router_cancel)?;
    if wire != reverse || shared_cancel.digest() != router_cancel.digest() {
        return Err("Router cancel changed the shared wire bytes or digest".into());
    }
    let shared_again: DeliveryCancel<BoardDeliveryTypes> = serde_json::from_slice(&reverse)?;
    if shared_cancel.digest() != shared_again.digest() {
        return Err("reverse cancel decoding changed the shared digest".into());
    }
    for encoded in [
        serde_json::to_string(&shared)?,
        serde_json::to_string(&router_cancel)?,
    ] {
        if encoded.contains("observation") || encoded.contains("DeliveryReceipt") {
            return Err("local observation appeared in the item/cancel wire envelope".into());
        }
    }
    Ok(())
}

#[test]
fn realistic_multi_claim_rejection_has_generous_local_observation_headroom() -> TestResult {
    // The real peer route returns every registry claim with this detail and
    // action. Sixteen long-labelled terminals exercise a busy development host.
    let claims = (0..16).map(|index| DeliveryPeerClaim {
        pid: 40_000 + index,
        name: Some(format!("🐒 Implementer Sidekick {index} · board storage, carrier recovery and independent native proof")),
        cwd: Some(format!("/workspace/engineering/platform/agent-collaboration/codex-router.feature-{index}/native-delivery-and-durable-interaction-store")),
    }).collect::<Vec<_>>();
    let receipt = DeliveryReceipt {
        outcome: DeliveryOutcome::Rejected(DeliveryRejection {
            native_failure: None,
            reason: DeliveryRejectionReason::LiveElsewhere,
            next_action: DeliveryNextAction::InspectTarget,
            client_code: None,
            detail: Some("this Claude session is claimed by 16 live terminals".into()),
            claims: Some(claims),
        }),
        reachability: Some(SessionReachability::ClaudeCodePeer),
        client: None,
    };
    let mut canonical = serde_json::to_value(RouterLocalObservation::from(receipt))?;
    canonical.sort_all_objects();
    let measured = serde_json::to_vec(&canonical)?.len();
    println!(
        "multi-claim rejection canonical bytes={measured}; local observation bound={}",
        RouterDeliveryTypes::MAX_LOCAL_OBSERVATION_BYTES
    );
    if measured >= usize::try_from(RouterDeliveryTypes::MAX_LOCAL_OBSERVATION_BYTES)? / 32 {
        return Err("realistic rejection lost the chosen observation guard's headroom".into());
    }
    Ok(())
}

#[test]
fn local_observation_wrapper_bound_is_measured_and_enforced_by_the_sdk() {
    use fleet_sync_delivery::{DeliverCall, LocalDelivery, LocalOutcome};
    let receipt = DeliveryReceipt {
        outcome: DeliveryOutcome::NotSubmitted {
            reason: String::new(),
            retryable: true,
        },
        reachability: None,
        client: None,
    };
    let receipt_bytes = serde_json::to_vec(&receipt).unwrap().len();
    let observation = RouterLocalObservation {
        receipt,
        scheduled_retry_at: Some(runtime_primitives::SyncTime::from_unix_millis(i64::MIN)),
        route_evidence: None,
    };
    let overhead = serde_json::to_vec(&observation).unwrap().len() - receipt_bytes;
    assert_eq!(overhead, 73);
    assert_eq!(
        RouterDeliveryTypes::MAX_LOCAL_OBSERVATION_BYTES,
        1024 * 1024 + 32 * 1024
    );
    let bound = usize::try_from(RouterDeliveryTypes::MAX_LOCAL_OBSERVATION_BYTES).unwrap();
    let base = serde_json::to_vec(&observation).unwrap().len();
    for (extra, expected_kind) in [(0, "observed"), (1, "uncertain")] {
        let mut observation = observation.clone();
        if let DeliveryOutcome::NotSubmitted { reason, .. } = &mut observation.receipt.outcome {
            *reason = "x".repeat(bound - base + extra);
        }
        assert_eq!(
            serde_json::to_vec(&observation).unwrap().len(),
            bound + extra
        );
        let answer = DeliverCall::<RouterDeliveryTypes>::validated_answer(LocalDelivery {
            outcome: LocalOutcome::Held { until: None },
            observation,
        });
        let encoded = serde_json::to_value(answer).unwrap();
        assert_eq!(
            encoded.pointer("/value/kind"),
            Some(&json!(expected_kind)),
            "{encoded}"
        );
    }
}

#[test]
fn maximal_escaped_provider_context_fits_the_declared_observation_allowance() {
    let route: collaboration_protocol::ProviderAcpRouteEvidence = serde_json::from_value(json!({
        "target": { "endpoint": { "serviceId": "00000000-0000-4000-8000-000000000001", "endpointId": "a".repeat(64) }, "sessionId": "\u{1}".repeat(4096) },
        "generation": { "serviceEpoch": "00000000-0000-4000-8000-000000000002", "generation": 9_007_199_254_740_991_u64 },
        "binding": "\u{1}".repeat(256),
        "attemptId": uuid::Uuid::now_v7().to_string(),
        "submission": "routerQueued", "settlement": "notObserved"
    })).unwrap();
    let receipt = DeliveryReceipt {
        outcome: DeliveryOutcome::Unknown,
        reachability: None,
        client: None,
    };
    let receipt_bytes = serde_json::to_vec(&receipt).unwrap().len();
    let observation = RouterLocalObservation {
        receipt,
        scheduled_retry_at: None,
        route_evidence: Some(route),
    };
    let overhead = serde_json::to_vec(&observation).unwrap().len() - receipt_bytes;
    println!("maximal escaped provider wrapper bytes={overhead}; allowance=32768");
    assert!(overhead <= 32 * 1024);
    let encoded = serde_json::to_value(
        fleet_sync_delivery::DeliverCall::<RouterDeliveryTypes>::validated_answer(
            fleet_sync_delivery::LocalDelivery {
                outcome: fleet_sync_delivery::LocalOutcome::Uncertain,
                observation,
            },
        ),
    )
    .unwrap();
    assert_eq!(encoded.pointer("/value/kind"), Some(&json!("observed")));
}
