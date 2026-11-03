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
fn decoder_refuses_the_well_formed_copy_to_registry_edge() -> serde_json::Result<()> {
    let bytes = object_envelope(
        json!({"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"}),
        "copy",
        "registry",
    )?;
    assert!(matches!(
        ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Registry),
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { last_seen: None }
        })
    ));
    Ok(())
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
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn decoder_allows_the_copy_to_fleet_edge() -> serde_json::Result<()> {
    let bytes = object_envelope(
        json!({"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"}),
        "copy",
        "fleet",
    )?;
    assert!(ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet).is_ok());
    Ok(())
}
