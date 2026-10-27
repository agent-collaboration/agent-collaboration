//! Literal wire examples pin the new mail kind without changing existing digest inputs.
use collaboration_mail_protocol::{
    CarrierAcceptance, MailBody, RemoteWakeRequest, RemoteWakeResult, WakeBody,
};
use serde_json::{Value, json};

fn target() -> Value {
    json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"fixture"})
}
const ID: &str = "019f0000-0000-7000-8000-000000000001";
#[test]
// Fixed independent wire examples must fail when decoding changes.
fn wake_send_and_every_control_round_trip_as_closed_typed_mail() {
    let mut creator = target();
    creator["kind"] = json!("session");
    let requests = vec![
        json!({"kind":"send","operationId":ID,"replace":false,"message":{"target":target(),"content":{"kind":"humanUser","text":"wake"},"delivery":"queue"},"timing":{"kind":"after","seconds":1},"expiry":{"kind":"after","seconds":120}}),
        json!({"kind":"show","wakeupId":ID}),
        json!({"kind":"pause","operationId":ID,"wakeupId":ID}),
        json!({"kind":"resume","operationId":ID,"wakeupId":ID}),
        json!({"kind":"cancel","operationId":ID,"wakeupId":ID}),
        json!({"kind":"wait","wakeupId":ID,"after":null,"deadline":"2026-10-23T22:00:00Z"}),
    ];
    for request in requests {
        let wire = json!({"kind":"wake","request":request,"requestedBy":creator});
        let body: MailBody = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(&body).unwrap(), wire);
        assert_eq!(body.kind(), collaboration_mail_protocol::MailKind::Wake);
        let mut unknown = wire;
        unknown["request"]["extra"] = json!(true);
        assert!(serde_json::from_value::<MailBody>(unknown).is_err());
    }
    assert!(
        serde_json::from_value::<RemoteWakeRequest>(
            json!({"kind":"show","wakeupId":"00000000-0000-4000-8000-000000000001"})
        )
        .is_err()
    );
    let body: WakeBody = serde_json::from_value(
        json!({"request":{"kind":"show","wakeupId":ID},"requestedBy":creator}),
    )
    .unwrap();
    let sender =
        collaboration_mail_protocol::MailSender::Session(serde_json::from_value(target()).unwrap());
    assert!(body.validate_sender(&sender).is_ok());
    assert!(
        body.validate_sender(&collaboration_mail_protocol::MailSender::HumanUser {
            installation: "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap()
        })
        .is_err()
    );
}

#[test]
// Fixed independent wire examples must fail when decoding changes.
fn typed_refusals_return_through_the_existing_acceptance_without_changing_old_variants() {
    for reason in [
        json!({"kind":"wakeExists","wakeup":ID,"creator":{"kind":"owner"}}),
        json!({"kind":"tooFarAhead","max":86400}),
        json!({"kind":"intervalTooShort","min":300}),
        json!({"kind":"lifetimeTooLong","max":604800}),
        json!({"kind":"expiryRequired"}),
        json!({"kind":"deadlineExpired"}),
    ] {
        let wire = json!({"wake":{"kind":"refused","value":{"reason":reason,"mutation":"none"}}});
        let acceptance: CarrierAcceptance = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(acceptance).unwrap(), wire);
    }
    for tag in [
        "started",
        "steered",
        "startedOrSteered",
        "queued",
        "peerMessageWritten",
    ] {
        let parsed: CarrierAcceptance = serde_json::from_value(json!(tag)).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(tag));
    }
    assert!(serde_json::from_value::<RemoteWakeResult>(json!({"kind":"refused","value":{"reason":{"kind":"wakeExists","wakeup":ID},"mutation":"none"}})).is_err());
}

#[test]
// Fixed independent wire examples must fail when decoding changes.
fn snapshot_mutation_and_wait_results_keep_their_complete_wire_shapes() {
    let snapshot = json!({
        "definition":{"wakeupId":ID,"changeId":ID,"createdBy":{"kind":"owner"},
        "message":{"target":target(),"content":{"kind":"humanUser","text":"wake"},"delivery":"auto"},
        "timing":{"kind":"after","seconds":60},"anchorAt":"2026-10-23T22:00:00.000Z","expiresAt":"2026-10-23T22:02:00Z","createdAt":"2026-10-23T22:00:00Z"},
        "state":"active","nextDueAt":"2026-10-23T22:01:00Z","firstFire":null,"pendingDeliveryId":null,"latestEventCursor":"owner-scoped cursor"
    });
    let mut missing = snapshot.clone();
    missing.as_object_mut().unwrap().remove("firstFire");
    assert!(
        serde_json::from_value::<collaboration_mail_protocol::RemoteWakeSnapshot>(missing).is_err()
    );
    for wire in [
        json!({"kind":"snapshot","value":snapshot}),
        json!({"kind":"mutation","value":{"wakeup":snapshot,"discardedDeliveryIds":[ID],"dispatchedDeliveries":[]}}),
        json!({"kind":"wait","value":{"wakeupId":ID,"outcome":{"kind":"timedOut"},"cursor":"owner-scoped cursor"}}),
    ] {
        let parsed: RemoteWakeResult = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    }
}

#[test]
// Fixed independent wire examples must fail when decoding changes.
fn remote_timing_preserves_exact_utc_and_required_nullable_outputs() {
    for text in ["2026-10-23T22:00:00.000Z", "2026-10-23T22:00:00.000123Z"] {
        let parsed: collaboration_mail_protocol::RemoteWakeTimestamp =
            serde_json::from_value(json!(text)).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), json!(text));
    }
    for text in ["2026-10-23T22:00:00+00:00", "not-a-time"] {
        assert!(
            serde_json::from_value::<collaboration_mail_protocol::RemoteWakeTimestamp>(json!(text))
                .is_err()
        );
    }
    assert!(
        serde_json::from_value::<collaboration_mail_protocol::RemoteWakeSeconds>(json!(0)).is_err()
    );
}
