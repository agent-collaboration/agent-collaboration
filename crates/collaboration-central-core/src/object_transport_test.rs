use super::*;
use serde_json::json;

fn object_envelope(
    source: serde_json::Value,
    source_face: &str,
    destination: &str,
) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "source": source,
        "sourceFace": source_face,
        "authority": if source_face == "session" { json!({"kind":"machine", "value":{"installation":"019f0000-0000-7000-8000-000000000807"}}) } else {json!({"kind":"object"})},
        "destination": {"kind": "fleet"},
        "destinationFace": destination,
        "identity": {"operation": "019f0000-0000-7000-8000-000000000806", "step": "object"},
        "kind": "central.installation-peer",
        "target": {"installation": "550e8400-e29b-41d4-a716-446655440806"},
        "request": null,
    }))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn decoder_allows_the_session_to_registry_edge() -> serde_json::Result<()> {
    let bytes = object_envelope(json!({"kind": "fleet"}), "session", "registry")?;
    assert!(ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Registry).is_ok());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Removed physical addresses and caller faces must fail decoding before dispatch."
)]
#[test]
fn removed_mailbox_addresses_and_faces_are_rejected_before_dispatch() -> serde_json::Result<()> {
    for (source, face) in [
        (
            json!({"kind":"mailbox","value":"019f0000-0000-7000-8000-000000000807"}),
            "registry",
        ),
        (json!({"kind":"fleet"}), "mailbox"),
    ] {
        let bytes = object_envelope(source, face, "copy")?;
        assert!(serde_json::from_slice::<ObjectCallEnvelope>(&bytes).is_err());
    }
    Ok(())
}
