use collaboration_protocol::{
    AttemptId, DeliveryCorrelationId, DeliveryNextAction, DeliveryOutcome, DeliveryPeerClaim,
    DeliveryRejection, DeliveryRejectionReason, SessionReachability,
};
use serde_json::{json, to_value};

#[test]
fn permission_differences_are_optional_closed_and_path_free() {
    // Arrange: a receipt and creation outcome written before difference reporting.
    let target = json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"thread"});
    let old = json!({"target":target,"generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1},"inputKind":"agent","representation":"declaredAgentText","clientUserMessageId":"push","resumeEffect":"accepted","acceptance":{"kind":"queueAccepted","submissionId":"submission"}});
    let created = json!({"kind":"created","operationId":"01900000-0000-7000-8000-000000000001","target":target});
    // Act/assert: old bytes decode and stay unchanged; new reports use only field classes.
    let decoded: collaboration_protocol::NativeSendReceipt =
        serde_json::from_value(old.clone()).unwrap();
    assert_eq!(to_value(decoded).unwrap(), old);
    let decoded: collaboration_protocol::ConversationCreateOutcome =
        serde_json::from_value(created.clone()).unwrap();
    assert_eq!(to_value(decoded).unwrap(), created);
    for fields in [
        json!(["profile", "sandbox", "writableRoots", "network"]),
        json!(["/private/path"]),
        json!(["unknown"]),
    ] {
        let allowed = fields == json!(["profile", "sandbox", "writableRoots", "network"]);
        let mut receipt = old.clone();
        receipt["permissionDifferences"] = fields.clone();
        let mut outcome = created.clone();
        outcome["permissionDifferences"] = fields;
        assert_eq!(
            serde_json::from_value::<collaboration_protocol::NativeSendReceipt>(receipt).is_ok(),
            allowed
        );
        assert_eq!(
            serde_json::from_value::<collaboration_protocol::ConversationCreateOutcome>(outcome)
                .is_ok(),
            allowed
        );
    }
}

#[test]
fn delivery_outcomes_and_reachability_round_trip_without_losing_strength() {
    for (outcome, expected) in [
        (DeliveryOutcome::Started, json!({"kind": "started"})),
        (DeliveryOutcome::Steered, json!({"kind": "steered"})),
        (
            DeliveryOutcome::StartedOrSteered,
            json!({"kind": "startedOrSteered"}),
        ),
        (DeliveryOutcome::Queued, json!({"kind": "queued"})),
        (
            DeliveryOutcome::PeerMessageWritten,
            json!({"kind": "peerMessageWritten"}),
        ),
        (
            DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason: "busy".to_owned(),
            },
            json!({"kind": "notSubmitted", "retryable": true, "reason": "busy"}),
        ),
        (
            DeliveryOutcome::Rejected(DeliveryRejection {
                reason: DeliveryRejectionReason::UnsupportedCapability,
                next_action: DeliveryNextAction::CorrectRequest,
                client_code: Some(-32601),
                detail: Some("unsupported".to_owned()),
                claims: None,
                native_failure: None,
            }),
            json!({"kind": "rejected", "reason": "unsupportedCapability", "nextAction":"correctRequest", "clientCode":-32601, "detail":"unsupported"}),
        ),
        (
            DeliveryOutcome::Rejected(DeliveryRejection {
                reason: DeliveryRejectionReason::LiveElsewhere,
                next_action: DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: Some("two terminals claim this session".to_owned()),
                claims: Some(vec![
                    DeliveryPeerClaim {
                        pid: 52304,
                        name: Some("terminal-one".to_owned()),
                        cwd: Some("/workspace/one".to_owned()),
                    },
                    DeliveryPeerClaim {
                        pid: 68833,
                        name: None,
                        cwd: None,
                    },
                ]),
                native_failure: None,
            }),
            json!({
                "kind":"rejected",
                "reason":"liveElsewhere",
                "nextAction":"inspectTarget",
                "clientCode":null,
                "detail":"two terminals claim this session",
                "claims":[
                    {"pid":52304,"name":"terminal-one","cwd":"/workspace/one"},
                    {"pid":68833,"name":null,"cwd":null}
                ]
            }),
        ),
        (DeliveryOutcome::Unknown, json!({"kind": "unknown"})),
    ] {
        let encoded = to_value(&outcome).expect("outcome encodes");
        assert_eq!(encoded, expected);
        let decoded: DeliveryOutcome = serde_json::from_value(encoded).expect("outcome decodes");
        assert_eq!(decoded, outcome);
    }

    for reachability in [
        SessionReachability::CodexAppServer,
        SessionReachability::ProviderAcp,
        SessionReachability::ClaudeCodePeer,
    ] {
        let encoded = to_value(reachability).expect("reachability encodes");
        let decoded: SessionReachability =
            serde_json::from_value(encoded).expect("reachability decodes");
        assert_eq!(decoded, reachability);
    }
}

#[test]
fn native_failure_is_optional_for_old_receipts_and_rejects_invalid_facts() {
    let old = json!({"reason":"unsupportedCapability","nextAction":"correctRequest","clientCode":-32601,"detail":null});
    let decoded: DeliveryRejection = serde_json::from_value(old.clone()).unwrap();
    assert!(decoded.native_failure.is_none());
    assert_eq!(to_value(decoded).unwrap(), old);

    let failure = json!({
        "operation":"thread/resume", "stage":"resume", "cause":"invalidResumeResponse",
        "generation":{"serviceEpoch":"00000000-0000-4000-8000-000000000001","generation":1},
        "resume":"accepted", "submission":"notDispatched"
    });
    let mut receipt = json!({"reason":"localRejection","nextAction":"inspectTarget","clientCode":null,"detail":"Recorded resume response failed validation","nativeFailure":failure});
    let decoded: DeliveryRejection = serde_json::from_value(receipt.clone()).unwrap();
    assert_eq!(to_value(decoded).unwrap(), receipt);
    for field in ["operation", "stage", "cause", "resume", "submission"] {
        let mut invalid = receipt.clone();
        invalid["nativeFailure"][field] = json!("unrecognized");
        assert!(
            serde_json::from_value::<DeliveryRejection>(invalid).is_err(),
            "{field}"
        );
    }
    receipt["nativeFailure"]
        .as_object_mut()
        .unwrap()
        .remove("generation");
    assert!(serde_json::from_value::<DeliveryRejection>(receipt).is_err());
}

#[test]
fn delivery_correlation_accepts_caller_values_and_allocates_uuidv7_values() {
    let attempt = AttemptId::generate();
    let correlation = DeliveryCorrelationId::generate();
    assert_ne!(attempt.as_str(), correlation.as_str());
    assert!(AttemptId::try_from(correlation.as_str().to_owned()).is_ok());
    assert!(AttemptId::try_from("not-a-uuid".to_owned()).is_err());
    let caller = DeliveryCorrelationId::try_from("caller-correlation".to_owned())
        .expect("caller correlation remains valid");
    assert_eq!(caller.as_str(), "caller-correlation");
    assert_eq!(
        serde_json::to_value(&caller).expect("encode"),
        "caller-correlation"
    );
    let decoded: DeliveryCorrelationId =
        serde_json::from_value(serde_json::json!("caller-correlation")).expect("decode");
    assert_eq!(decoded, caller);
    for invalid in [
        String::new(),
        "bad\0correlation".to_owned(),
        "x".repeat(4097),
    ] {
        assert!(DeliveryCorrelationId::try_from(invalid).is_err());
    }
}
