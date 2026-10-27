// Compile the exact Worker codec, without copying its implementation. These
// native checks cover validation; the actual workers-rs attachment path is a
// separate workerd proof.
#[path = "../../collaboration-central-cloudflare/src/binding_attachment.rs"]
mod binding_attachment;

use binding_attachment::BindingAttachment;
use collaboration_central_core::AdmissionOrigin;
use fleet_sync_protocol::{AttachSequence, ConnectionId, PeerId, SchemaSupport, SessionId};
use fleet_sync_session::ConnectionRegistration;
use runtime_primitives::SyncTime;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn attachment(
    max_version: u64,
    attach_seq: u64,
    expires_at: i64,
) -> Result<BindingAttachment, Box<dyn std::error::Error + Send + Sync>> {
    Ok(BindingAttachment {
        connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
        peer: PeerId::new(durable_steps::Uuid::now_v7())?,
        expires_at: SyncTime::from_unix_millis(expires_at),
        registration: ConnectionRegistration::Attaching {
            session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
            schema_support: SchemaSupport { max_version },
            attach_seq: AttachSequence::new(attach_seq)?,
        },
        admission_origin: AdmissionOrigin {
            installation: message_board::ServiceId::try_from(
                "550e8400-e29b-41d4-a716-446655440000".to_owned(),
            )?,
            machine: message_board::MachineLabel::try_from("admitted machine 🦊".to_owned())?,
        },
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn full_integer_domains_roundtrip_as_decimal_text_without_losing_admission_origin() -> TestResult {
    for (support, sequence, expiry) in [
        (0, 1, 0),
        (9_007_199_254_740_993, 9_007_199_254_740_993, i64::MIN),
        (u64::MAX, u64::MAX, i64::MAX),
    ] {
        let expected = attachment(support, sequence, expiry)?;
        let wire = serde_json::to_value(&expected)?;
        assert_eq!(
            wire.pointer("/registration/schemaSupport"),
            Some(&serde_json::json!(support.to_string()))
        );
        assert_eq!(
            wire.pointer("/registration/attachSeq"),
            Some(&serde_json::json!(sequence.to_string()))
        );
        assert_eq!(
            wire.get("expiresAt"),
            Some(&serde_json::json!(expiry.to_string()))
        );
        let restored: BindingAttachment = serde_json::from_value(wire)?;
        assert_eq!(restored, expected);
        assert_eq!(restored.binding().connection, expected.connection);
        assert_eq!(restored.binding().peer, expected.peer);
        assert_eq!(restored.binding().expires_at, expected.expires_at);
    }
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn malformed_noncanonical_and_out_of_domain_attachments_are_refused() -> TestResult {
    let valid = serde_json::to_value(attachment(7, 1, 123)?)?;
    for field in ["schemaSupport", "attachSeq"] {
        for invalid in [
            serde_json::json!(7),
            serde_json::json!("+7"),
            serde_json::json!("07"),
            serde_json::json!("-1"),
            serde_json::json!("18446744073709551616"),
        ] {
            let mut wire = valid.clone();
            *wire
                .get_mut("registration")
                .and_then(|registration| registration.get_mut(field))
                .ok_or("missing registration field")? = invalid;
            assert!(
                serde_json::from_value::<BindingAttachment>(wire).is_err(),
                "accepted invalid {field}"
            );
        }
    }
    let mut zero_sequence = valid.clone();
    *zero_sequence
        .pointer_mut("/registration/attachSeq")
        .ok_or("attachSeq missing")? = serde_json::json!("0");
    assert!(serde_json::from_value::<BindingAttachment>(zero_sequence).is_err());
    for invalid in [
        serde_json::json!(123),
        serde_json::json!("0123"),
        serde_json::json!("9223372036854775808"),
    ] {
        let mut wire = valid.clone();
        *wire.get_mut("expiresAt").ok_or("expiresAt missing")? = invalid;
        assert!(serde_json::from_value::<BindingAttachment>(wire).is_err());
    }
    let mut extra = valid;
    extra
        .get_mut("registration")
        .and_then(serde_json::Value::as_object_mut)
        .ok_or("registration missing")?
        .insert("unexpected".into(), serde_json::json!(true));
    assert!(serde_json::from_value::<BindingAttachment>(extra).is_err());
    Ok(())
}
