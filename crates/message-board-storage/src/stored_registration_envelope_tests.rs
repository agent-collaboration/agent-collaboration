//! S1's private storage representation keeps immutable preparation and lifecycle
//! together, and constructor validation remains real on every decode.
//! No pre-fix semantic red: these records previously had no Serde codec.
use super::tests::{confirmed, preparation, registration};
use super::*;
use serde_json::json;

fn record() -> FleetSessionRegistrationRecord {
    let preparation = preparation();
    FleetSessionRegistrationRecord::new(
        preparation.clone(),
        FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(&preparation),
        },
    )
    .unwrap()
}

#[test]
fn stored_registration_envelope_keeps_the_version_and_all_immutable_preparation_facts() {
    let record = record();

    let encoded = serde_json::to_value(&record).unwrap();
    let decoded: FleetSessionRegistrationRecord = serde_json::from_value(encoded.clone()).unwrap();

    assert_eq!(encoded["version"], "1");
    assert_eq!(
        encoded["preparation"],
        json!({
            "registrationId":"019f0000-0000-7000-8000-000000000001",
            "operationId":"019f0000-0000-7000-8000-000000000002",
            "installation":"019f0000-0000-7000-8000-000000000003",
            "bindingAddress":"http://127.0.0.1:8788/mcp/session/0123456789abcdef0123456789abcdef",
            "launch":{
                "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
                "projectId":"019f0000-0000-7000-8000-000000000005",
                "checkedWorkspace":"019f0000-0000-7000-8000-000000000004",
                "startedBy":{"kind":"routerOperator"},
                "isolation":{"kind":"host","instanceId":"019f0000-0000-7000-8000-000000000006"},
                "launchContext":{"workingDirectory":"/fixture/project"},
                "machineEnvironment":"physical"
            }
        })
    );
    assert_eq!(encoded["lifecycle"]["state"], "active");
    assert_eq!(decoded, record);
    assert!(!format!("{decoded:?}").contains("0123456789abcdef0123456789abcdef"));
}

#[test]
fn stored_registration_envelope_roundtrips_each_closed_lifecycle_without_losing_preparation() {
    let preparation = preparation();
    let registration = registration(&preparation);
    let session = registration.session.clone();
    let lifecycles = [
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::NotDispatched {},
        },
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Dispatched {},
        },
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::CreatedPendingSetup {
                session: session.clone(),
            },
        },
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::Created {
                registration: Box::new(registration.clone()),
            },
        },
        FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(&preparation),
        },
        FleetSessionBindingLifecycle::Retired {
            confirmed: confirmed(&preparation),
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::NotCreated {},
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::KnownCreated {
                session,
                cause: SessionSetupAbandonmentCause::SetupInterrupted {},
            },
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::RegistrationRefused {
                registration: Box::new(registration),
            },
        },
    ];
    for lifecycle in lifecycles {
        let original = FleetSessionRegistrationRecord::new(preparation.clone(), lifecycle).unwrap();

        let encoded = serde_json::to_string(&original).unwrap();
        let decoded: FleetSessionRegistrationRecord = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, original);
    }
}

#[test]
fn stored_registration_envelope_refuses_unknown_versions_fields_and_invalid_domain_values() {
    let original = serde_json::to_value(record()).unwrap();
    for (pointer, value) in [
        ("/version", json!("2")),
        ("/version", json!(1)),
        ("/preparation/registrationId", json!("invalid")),
        ("/preparation/operationId", json!("invalid")),
        (
            "/preparation/bindingAddress",
            json!("http://127.0.0.1:0/mcp/session/0123456789abcdef0123456789abcdef"),
        ),
        ("/preparation/launch/projectId", json!("invalid")),
        ("/preparation/launch/machineEnvironment", json!("future")),
        ("/lifecycle/state", json!("future")),
    ] {
        let mut corrupted = original.clone();
        *corrupted.pointer_mut(pointer).unwrap() = value;

        assert!(
            serde_json::from_value::<FleetSessionRegistrationRecord>(corrupted).is_err(),
            "accepted invalid {pointer}"
        );
    }
    for pointer in ["", "/preparation", "/preparation/launch", "/lifecycle"] {
        let mut corrupted = original.clone();
        corrupted
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".to_owned(), json!(true));

        assert!(
            serde_json::from_value::<FleetSessionRegistrationRecord>(corrupted).is_err(),
            "accepted extra field at {pointer}"
        );
    }
    let mut unversioned = original;
    unversioned.as_object_mut().unwrap().remove("version");
    assert!(serde_json::from_value::<FleetSessionRegistrationRecord>(unversioned).is_err());
}

#[test]
fn stored_registration_envelope_routes_scope_and_lifecycle_mismatches_through_the_constructor() {
    let original = serde_json::to_value(record()).unwrap();
    for (pointer, value, field) in [
        (
            "/preparation/installation",
            json!("019f0000-0000-7000-8000-000000000099"),
            "installation",
        ),
        (
            "/preparation/launch/projectId",
            json!("019f0000-0000-7000-8000-000000000099"),
            "registration",
        ),
        (
            "/preparation/launch/machineEnvironment",
            json!("cloudVm"),
            "registration",
        ),
        (
            "/lifecycle/confirmed/workspaceId",
            json!("019f0000-0000-7000-8000-000000000099"),
            "workspace",
        ),
    ] {
        let mut corrupted = original.clone();
        *corrupted.pointer_mut(pointer).unwrap() = value;

        let error =
            serde_json::from_value::<FleetSessionRegistrationRecord>(corrupted).unwrap_err();

        assert!(error.to_string().contains(field), "{error}");
        assert!(
            !error
                .to_string()
                .contains("0123456789abcdef0123456789abcdef")
        );
    }
}
