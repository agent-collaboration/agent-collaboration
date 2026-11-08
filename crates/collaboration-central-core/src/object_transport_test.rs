use super::*;
use serde_json::json;
fn organization() -> fleet_identity_protocol::OrganizationId {
    "019f0000-0000-7000-8000-00000000ac01"
        .to_owned()
        .try_into()
        .expect("fixed UUIDv7 organization")
}
fn fleet() -> ObjectAddress {
    ObjectAddress::Fleet {
        organization: organization(),
    }
}

fn object_envelope(
    source: serde_json::Value,
    source_face: &str,
    destination: &str,
) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "organization": organization(),
        "source": source,
        "sourceFace": source_face,
        "authority": if source_face == "session" { json!({"kind":"machine", "value":{"installation":"019f0000-0000-7000-8000-000000000807"}}) } else {json!({"kind":"object"})},
        "destination": {"kind": "fleet", "value": {"organization": organization()}},
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
        ObjectCallEnvelope::decode(&bytes, fleet(), CallerKind::Registry, &organization()),
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
    let bytes = object_envelope(
        json!({"kind": "fleet", "value": {"organization": organization()}}),
        "session",
        "registry",
    )?;
    assert!(
        ObjectCallEnvelope::decode(&bytes, fleet(), CallerKind::Registry, &organization()).is_ok()
    );
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
    assert!(
        ObjectCallEnvelope::decode(&bytes, fleet(), CallerKind::Fleet, &organization()).is_err()
    );
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
            json!({"kind":"fleet","value":{"organization": organization()}}),
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
            json!({"kind":"fleet","value":{"organization": organization()}}),
            "registry",
            json!({"kind":"object"}),
            true,
        ),
        (
            json!({"kind":"fleet","value":{"organization": organization()}}),
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
            ObjectCallEnvelope::decode(&bytes, address, CallerKind::Copy, &organization()).is_ok(),
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
        (
            json!({"kind":"fleet","value":{"organization": organization()}}),
            "mailbox",
        ),
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
    assert!(
        ObjectCallEnvelope::decode(&bytes, fleet(), CallerKind::Fleet, &organization()).is_ok()
    );
    for kind in [
        "central.admin-import-workflow",
        "central.fleet-request-effects",
        "central.copy-operations",
    ] {
        call["kind"] = json!(kind);
        assert!(
            ObjectCallEnvelope::decode(
                &serde_json::to_vec(&call)?,
                fleet(),
                CallerKind::Fleet,
                &organization()
            )
            .is_err()
        );
    }
    Ok(())
}

// Each case keeps a valid DAG edge and varies only the organization binding.
// A decoder that accepts a body from another Fleet would pass it to the wrong store.
#[test]
fn organization_binding_refuses_a_foreign_fleet_before_dispatch() {
    let bytes = object_envelope(
        json!({"kind":"fleet","value":{"organization":organization()}}),
        "session",
        "registry",
    )
    .unwrap();
    let foreign: fleet_identity_protocol::OrganizationId = "019f0000-0000-7000-8000-00000000ac02"
        .to_owned()
        .try_into()
        .unwrap();
    let address = ObjectAddress::Fleet {
        organization: foreign.clone(),
    };
    let mut body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    body["destination"] = serde_json::to_value(&address).unwrap();
    assert!(
        ObjectCallEnvelope::decode(
            &serde_json::to_vec(&body).unwrap(),
            address,
            CallerKind::Registry,
            &foreign
        )
        .is_err()
    );
}

#[test]
fn organization_binding_refuses_global_and_missing_organization_shapes() {
    let bytes = object_envelope(
        json!({"kind":"fleet","value":{"organization":organization()}}),
        "session",
        "registry",
    )
    .unwrap();
    let mut body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    body.as_object_mut().unwrap().remove("organization");
    assert!(serde_json::from_value::<ObjectCallEnvelope>(body).is_err());
    assert!(serde_json::from_value::<ObjectAddress>(json!({"kind":"fleet"})).is_err());
}

#[test]
fn organization_binding_refuses_encoding_a_cross_fleet_dispatch() {
    let caller = CallerContext::<crate::SessionKind>::for_machine(
        organization(),
        "019f0000-0000-7000-8000-00000000ac04"
            .to_owned()
            .try_into()
            .unwrap(),
    );
    let foreign: fleet_identity_protocol::OrganizationId = "019f0000-0000-7000-8000-00000000ac02"
        .to_owned()
        .try_into()
        .unwrap();
    let call = Dispatch::<crate::RegistryKind> {
        call_id: crate::ObjectCallId::new("019f0000-0000-7000-8000-00000000ac03").unwrap(),
        to: crate::FleetAddress::new(foreign),
        call: crate::RegistryCall::RevokeEnrollment {
            installation: "019f0000-0000-7000-8000-00000000ac05"
                .to_owned()
                .try_into()
                .unwrap(),
        },
    };
    assert!(serialize_object_call(&caller, call).is_err());
}

/// Every Access-era public route builds its caller with
/// `CentralApplication`'s legacy organization (owner or Access machine) and
/// can only name the legacy Fleet. Program Design step 2 removes these routes.
fn access_era_callers(
    legacy: &fleet_identity_protocol::OrganizationId,
) -> Vec<CallerContext<crate::EdgeKind>> {
    vec![
        CallerContext::for_owner(
            crate::OwnerIdentity::from_access(crate::AccessCaller::Owner {
                email: "owner@example.invalid".into(),
            })
            .unwrap(),
            legacy.clone(),
        ),
        CallerContext::for_verified_edge(
            crate::AccessCaller::Machine {
                client_id: crate::AccessClientId::new("machine-client.access").unwrap(),
            },
            legacy.clone(),
        )
        .unwrap(),
    ]
}

fn admin_input() -> crate::person_request::PersonRequestInput {
    crate::person_request::PersonRequestInput {
        route: crate::person_request::AdminRoute::WorkspaceLookup,
        method: "POST".into(),
        authorization: "DPoP token".into(),
        proof: "proof".into(),
        body: "{}".into(),
    }
}

fn call_identity() -> durable_steps::CallId {
    durable_steps::CallId {
        operation: durable_steps::OperationId::new(
            durable_steps::Uuid::parse_str("019f0000-0000-7000-8000-00000000ac06").unwrap(),
        )
        .unwrap(),
        step: durable_steps::StepName::new("access_era").unwrap(),
    }
}

// Wrong build caught: an Access-era caller (owner or Access machine, bound to
// the legacy organization) reaching a non-legacy Fleet object, which an
// unstamped organization would then answer. Each encoder refuses the
// crossing, and a hand-built crossing envelope is refused at decode; the same
// envelope addressed to the legacy Fleet decodes (positive control).
#[test]
fn access_era_callers_reach_only_the_legacy_fleet() {
    let legacy = organization();
    let other: fleet_identity_protocol::OrganizationId = "019f0000-0000-7000-8000-00000000ac02"
        .to_owned()
        .try_into()
        .unwrap();
    for caller in access_era_callers(&legacy) {
        assert!(
            crate::object_call_envelope::encode_shared_call::<crate::EdgeKind, FleetKind, _>(
                &caller,
                crate::FleetAddress::new(other.clone()),
                crate::FleetOrganizationAdmin,
                admin_input(),
                call_identity(),
            )
            .is_err(),
            "the shared encoder refuses an edge call to another organization's Fleet"
        );
        assert!(
            serialize_object_call(
                &caller,
                Dispatch::<crate::RegistryKind> {
                    call_id: crate::ObjectCallId::new("019f0000-0000-7000-8000-00000000ac03")
                        .unwrap(),
                    to: crate::FleetAddress::new(other.clone()),
                    call: crate::RegistryCall::RevokeEnrollment {
                        installation: "019f0000-0000-7000-8000-00000000ac05"
                            .to_owned()
                            .try_into()
                            .unwrap(),
                    },
                },
            )
            .is_err(),
            "the typed encoder refuses an edge call to another organization's registry"
        );
        let legacy_call =
            crate::object_call_envelope::encode_shared_call::<crate::EdgeKind, FleetKind, _>(
                &caller,
                crate::FleetAddress::new(legacy.clone()),
                crate::FleetOrganizationAdmin,
                admin_input(),
                call_identity(),
            )
            .unwrap();
        let bytes = legacy_call.bytes().to_vec();
        assert!(
            ObjectCallEnvelope::decode(&bytes, fleet(), CallerKind::Fleet, &legacy).is_ok(),
            "positive control: the legacy Fleet accepts its own Access-era caller"
        );
        let mut crossing: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let foreign = ObjectAddress::Fleet {
            organization: other.clone(),
        };
        crossing["destination"] = serde_json::to_value(&foreign).unwrap();
        assert!(
            ObjectCallEnvelope::decode(
                &serde_json::to_vec(&crossing).unwrap(),
                foreign,
                CallerKind::Fleet,
                &other
            )
            .is_err(),
            "a non-legacy Fleet refuses a caller bound to the legacy organization"
        );
    }
}
