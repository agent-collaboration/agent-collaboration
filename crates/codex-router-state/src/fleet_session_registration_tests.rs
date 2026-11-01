//! The row's constructor catches corrupt launch/scope combinations before persistence exists.
use super::*;
use serde_json::json;

const CAPABILITY: &str = "0123456789abcdef0123456789abcdef";
const WORKSPACE: &str = "019f0000-0000-7000-8000-000000000004";

#[test]
fn fleet_session_setup_pending_and_local_refusals_retain_the_actual_id_as_closed_payloads() {
    let preparation = preparation();
    let session = serde_json::to_value(registration(&preparation).session).unwrap();
    let mut payloads = vec![json!({"state":"prepared","creation":{
        "kind":"createdPendingSetup","session":session
    }})];
    for cause in [
        "modelMismatch",
        "effectiveWorkingDirectoryMismatch",
        "workspaceRootsMismatch",
        "setupInterrupted",
    ] {
        payloads.push(json!({"state":"abandoned","creation":{
            "kind":"knownCreated","session":session,"cause":{"kind":cause}
        }}));
    }
    for setting in ["mode", "model", "effort"] {
        payloads.push(json!({"state":"abandoned","creation":{
            "kind":"knownCreated","session":session,"cause":{"kind":"initialSettingRefused","setting":setting}
        }}));
    }
    for payload in payloads {
        let lifecycle: FleetSessionBindingLifecycle =
            serde_json::from_value(payload.clone()).unwrap();
        let record = FleetSessionRegistrationRecord::new(preparation.clone(), lifecycle).unwrap();
        assert_eq!(serde_json::to_value(record.lifecycle()).unwrap(), payload);
        assert!(
            record.lifecycle().registration().is_none(),
            "local setup must not become a registration-ready request"
        );
    }
    for cause in [
        json!({"kind":"networkFailure"}),
        json!({"kind":"initialSettingRefused","setting":"future"}),
        json!({"kind":"initialSettingRefused","setting":"mode","value":"raw-setting"}),
        json!({"kind":"modelMismatch","extra":true}),
    ] {
        assert!(
            serde_json::from_value::<FleetSessionBindingLifecycle>(
                json!({"state":"abandoned","creation":{
                    "kind":"knownCreated","session":session,"cause":cause
                }})
            )
            .is_err()
        );
    }
}

#[test]
fn fleet_session_setup_known_id_must_belong_to_the_prepared_endpoint() {
    let preparation = preparation();
    let mut session = registration(&preparation).session;
    session.endpoint.endpoint_id = "other-provider".to_owned().try_into().unwrap();
    for lifecycle in [
        FleetSessionBindingLifecycle::Prepared {
            creation: PreparedSessionCreation::CreatedPendingSetup {
                session: session.clone(),
            },
        },
        FleetSessionBindingLifecycle::Abandoned {
            creation: AbandonedSessionCreation::KnownCreated {
                session,
                cause: SessionSetupAbandonmentCause::SetupInterrupted {},
            },
        },
    ] {
        assert_eq!(
            FleetSessionRegistrationRecord::new(preparation.clone(), lifecycle)
                .unwrap_err()
                .field,
            "session"
        );
    }
}

fn preparation() -> FleetSessionPreparation {
    FleetSessionPreparation {
        registration_id: "019f0000-0000-7000-8000-000000000001".to_owned().try_into().unwrap(),
        operation_id: serde_json::from_value(json!("019f0000-0000-7000-8000-000000000002")).unwrap(),
        installation: "019f0000-0000-7000-8000-000000000003".to_owned().try_into().unwrap(),
        binding_address: format!("http://127.0.0.1:8788/mcp/session/{CAPABILITY}").try_into().unwrap(),
        launch: serde_json::from_value(json!({
            "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
            "projectId":"019f0000-0000-7000-8000-000000000005", "checkedWorkspace":WORKSPACE,
            "startedBy":{"kind":"routerOperator"},
            "isolation":{"kind":"host","instanceId":"019f0000-0000-7000-8000-000000000006"},
            "launchContext":{"workingDirectory":"/fixture/project"}, "machineEnvironment":"physical"
        })).unwrap(),
    }
}
fn registration(preparation: &FleetSessionPreparation) -> RegisterFleetSession {
    preparation.launch.registration(serde_json::from_value(json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},"sessionId":"provider-returned-conversation"
    })).unwrap())
}
fn confirmed(preparation: &FleetSessionPreparation) -> RegisteredFleetSession {
    RegisteredFleetSession {
        registration: registration(preparation),
        organization_id: "019f0000-0000-7000-8000-000000000007"
            .to_owned()
            .try_into()
            .unwrap(),
        workspace_id: WORKSPACE.to_owned().try_into().unwrap(),
    }
}

#[test]
fn fleet_session_binding_addresses_refuse_zero_port_and_hide_capabilities_in_diagnostics() {
    let preparation = preparation();
    assert_eq!(
        preparation.binding_address.origin().unwrap().as_str(),
        "http://127.0.0.1:8788"
    );
    assert!(!format!("{preparation:?}").contains(CAPABILITY));
    assert_eq!(
        format!("{:?}", preparation.binding_address),
        "SessionBindingAddress([REDACTED])"
    );
    for address in [
        format!("http://127.0.0.1:0/mcp/session/{CAPABILITY}"),
        format!("http://user:secret@127.0.0.1:8788/mcp/session/{CAPABILITY}"),
        format!("http://127.0.0.1:8788/mcp/session/{CAPABILITY}?token=secret"),
        format!("http://127.0.0.1:8788/mcp/session/{CAPABILITY}#fragment"),
        "http://127.0.0.1:8788/mcp/session/short".to_owned(),
    ] {
        let error = SessionBindingAddress::try_from(address).unwrap_err();
        assert!(!error.to_string().contains(CAPABILITY));
        assert!(!error.to_string().contains("secret"));
    }
    assert!(SessionBindingOrigin::try_from("http://127.0.0.1:0".to_owned()).is_err());
    assert!(SessionBindingOrigin::try_from("http://127.0.0.1:8788/mcp".to_owned()).is_err());
    assert_ne!(
        preparation.binding_address.origin().unwrap(),
        SessionBindingOrigin::try_from("http://127.0.0.1:8789".to_owned()).unwrap()
    );
}

#[test]
fn fleet_session_registration_refuses_a_foreign_installation_and_changed_immutable_launch_facts() {
    let preparation = preparation();
    let mut foreign = preparation.clone();
    foreign.installation = "019f0000-0000-7000-8000-000000000099"
        .to_owned()
        .try_into()
        .unwrap();
    assert_eq!(
        FleetSessionRegistrationRecord::new(
            foreign,
            FleetSessionBindingLifecycle::Prepared {
                creation: PreparedSessionCreation::NotDispatched {}
            }
        )
        .unwrap_err()
        .field,
        "installation"
    );
    let original = serde_json::to_value(registration(&preparation)).unwrap();
    for (field, value) in [
        ("projectId", json!("019f0000-0000-7000-8000-000000000098")),
        ("machineEnvironment", json!("cloudVm")),
        (
            "isolation",
            json!({"kind":"container","instanceId":"019f0000-0000-7000-8000-000000000006"}),
        ),
        (
            "session",
            json!({"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"claude-local"},"sessionId":"provider-returned-conversation"}),
        ),
    ] {
        let mut value_changed = original.clone();
        value_changed[field] = value;
        let changed: RegisterFleetSession = serde_json::from_value(value_changed).unwrap();
        assert_eq!(
            FleetSessionRegistrationRecord::new(
                preparation.clone(),
                FleetSessionBindingLifecycle::Prepared {
                    creation: PreparedSessionCreation::Created {
                        registration: Box::new(changed)
                    }
                }
            )
            .unwrap_err()
            .field,
            "registration"
        );
    }
}

#[test]
fn fleet_session_registration_scope_matches_the_workspace_checked_before_creation() {
    let preparation = preparation();
    let mut wrong_scope = confirmed(&preparation);
    wrong_scope.workspace_id = "019f0000-0000-7000-8000-000000000098"
        .to_owned()
        .try_into()
        .unwrap();
    for lifecycle in [
        FleetSessionBindingLifecycle::Active {
            confirmed: wrong_scope.clone(),
        },
        FleetSessionBindingLifecycle::Retired {
            confirmed: wrong_scope,
        },
    ] {
        assert_eq!(
            FleetSessionRegistrationRecord::new(preparation.clone(), lifecycle)
                .unwrap_err()
                .field,
            "workspace"
        );
    }
    let active = FleetSessionRegistrationRecord::new(
        preparation.clone(),
        FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(&preparation),
        },
    )
    .unwrap();
    assert_eq!(
        active.lifecycle().registration(),
        Some(&registration(&preparation))
    );
    assert!(!format!("{active:?}").contains(CAPABILITY));
}

#[test]
fn fleet_session_registration_lifecycle_is_closed_and_prepared_contains_no_invented_session() {
    let preparation = preparation();
    let pending = FleetSessionBindingLifecycle::Prepared {
        creation: PreparedSessionCreation::Dispatched {},
    };
    assert!(pending.registration().is_none());
    assert_eq!(
        serde_json::to_value(&pending).unwrap(),
        json!({"state":"prepared","creation":{"kind":"dispatched"}})
    );
    for mut wire in [
        serde_json::to_value(&pending).unwrap(),
        serde_json::to_value(FleetSessionBindingLifecycle::Active {
            confirmed: confirmed(&preparation),
        })
        .unwrap(),
    ] {
        let parsed: FleetSessionBindingLifecycle = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
        wire["unexpected"] = json!(true);
        assert!(serde_json::from_value::<FleetSessionBindingLifecycle>(wire).is_err());
    }
    assert!(
        serde_json::from_value::<FleetSessionBindingLifecycle>(
            json!({"state":"prepared","creation":{"kind":"dispatched","sessionId":"invented"}})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<FleetSessionBindingLifecycle>(json!({"state":"active"})).is_err()
    );
}
