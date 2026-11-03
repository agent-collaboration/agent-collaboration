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
fn decoder_refuses_a_synchronous_copy_to_fleet_call() -> serde_json::Result<()> {
    let bytes = object_envelope(
        json!({"kind": "copy", "value": "019f0000-0000-7000-8000-000000000806"}),
        "copy",
        "fleet",
    )?;
    assert!(ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet).is_err());
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions reject the forbidden source at the real envelope decoder."
)]
#[test]
fn project_calls_require_the_fleet_physical_owner() -> serde_json::Result<()> {
    // The C3 oracle is physical ownership, independent of the logical Fleet facet.
    let project = "019f0000-0000-7000-8000-000000000806";
    for (source, face, authority, expected) in [
        (
            json!({"kind":"fleet"}),
            "edge",
            json!({"kind":"owner","value":"fixture@example.test"}),
            false,
        ),
        (
            json!({"kind":"copy","value":project}),
            "copy",
            json!({"kind":"object"}),
            false,
        ),
        (
            json!({"kind":"fleet"}),
            "registry",
            json!({"kind":"object"}),
            true,
        ),
        (
            json!({"kind":"fleet"}),
            "session",
            json!({"kind":"machine","value":{"installation":"019f0000-0000-7000-8000-000000000807"}}),
            true,
        ),
    ] {
        let mut call: serde_json::Value =
            serde_json::from_slice(&object_envelope(source, face, "copy")?)?;
        call["authority"] = authority;
        call["destination"] = json!({"kind":"copy","value":project});
        let _: ObjectCallEnvelope = serde_json::from_value(call.clone())?;
        let bytes = serde_json::to_vec(&call)?;
        let address = ObjectAddress::Copy(
            project
                .to_owned()
                .try_into()
                .map_err(serde::de::Error::custom)?,
        );
        assert_eq!(
            ObjectCallEnvelope::decode(&bytes, address, CallerKind::Copy).is_ok(),
            expected,
            "source facet: {face}"
        );
    }
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

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions distinguish the journal delivery contract from a synchronous return edge."
)]
#[test]
fn a_project_can_deliver_only_its_durable_directory_summary_to_fleet() -> serde_json::Result<()> {
    let mut call: serde_json::Value = serde_json::from_slice(&object_envelope(
        json!({"kind":"copy","value":"019f0000-0000-7000-8000-000000000806"}),
        "copy",
        "fleet",
    )?)?;
    call["kind"] = json!("central.directory-summary");
    let bytes = serde_json::to_vec(&call)?;
    assert!(ObjectCallEnvelope::decode(&bytes, ObjectAddress::Fleet, CallerKind::Fleet).is_ok());
    for kind in [
        "central.admin-import-workflow",
        "central.fleet-request-effects",
        "central.copy-operations",
    ] {
        call["kind"] = json!(kind);
        assert!(
            ObjectCallEnvelope::decode(
                &serde_json::to_vec(&call)?,
                ObjectAddress::Fleet,
                CallerKind::Fleet
            )
            .is_err()
        );
    }
    Ok(())
}
