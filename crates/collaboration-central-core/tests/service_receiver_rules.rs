//! C7 policy consumes typed authenticated facts, never a body-selected action.
#![cfg(test)]
use collaboration_central_core::{
    PinnedServiceCaller, RegisteredServiceClientKind, ServiceCallerIdentity, ServiceClientId,
    ServiceOperation, ServiceOperationTrust, ServiceReceiverHome, ServiceReceiverPolicy,
    ServiceReceiverRefusal, ServiceTokenFacts, ServiceTokenTimeRefusal, SignatureValidity,
    identity_contract::ResourceUri,
};
use fleet_identity_protocol::{OrganizationId, UnixSeconds};

const OPERATIONS: [ServiceOperation; 9] = [
    ServiceOperation::AdmitSignIn,
    ServiceOperation::PersonStanding,
    ServiceOperation::CentralLookup,
    ServiceOperation::DeviceLoginDecisions,
    ServiceOperation::MachineTokenDecisions,
    ServiceOperation::DeviceApprovalAuthorize,
    ServiceOperation::DeviceApprovalActivate,
    ServiceOperation::DeviceApprovalCancel,
    ServiceOperation::InitializeOrganization,
];
const KINDS: [RegisteredServiceClientKind; 6] = [
    RegisteredServiceClientKind::FleetDirectory,
    RegisteredServiceClientKind::FleetCentral,
    RegisteredServiceClientKind::Agent,
    RegisteredServiceClientKind::Person,
    RegisteredServiceClientKind::Machine,
    RegisteredServiceClientKind::DeviceLogin,
];

fn uri(value: &str) -> ResourceUri {
    ResourceUri::try_from(value.to_owned()).unwrap()
}
fn organization(last: &str) -> OrganizationId {
    OrganizationId::try_from(format!("019f0000-0000-7000-8000-0000000000{last}")).unwrap()
}
fn client(value: &str) -> ServiceClientId {
    ServiceClientId::try_from(value.to_owned()).unwrap()
}
fn is_directory_call(operation: ServiceOperation) -> bool {
    matches!(
        operation,
        ServiceOperation::DeviceApprovalAuthorize
            | ServiceOperation::DeviceApprovalActivate
            | ServiceOperation::DeviceApprovalCancel
            | ServiceOperation::InitializeOrganization
    )
}
fn home(operation: ServiceOperation) -> ServiceReceiverHome {
    match operation {
        ServiceOperation::CentralLookup | ServiceOperation::DeviceLoginDecisions => {
            ServiceReceiverHome::FleetDirectory
        }
        _ => ServiceReceiverHome::Central {
            organization_id: organization("01"),
        },
    }
}
fn trust(operation: ServiceOperation) -> ServiceOperationTrust {
    ServiceOperationTrust {
        operation,
        audience: uri(&format!(
            "https://central.example.invalid/{}",
            operation.scope()
        )),
        caller: if is_directory_call(operation) {
            PinnedServiceCaller::FleetDirectory {
                client_id: client("pinned-directory"),
            }
        } else {
            PinnedServiceCaller::Issuer
        },
    }
}
fn policy(operation: ServiceOperation) -> ServiceReceiverPolicy {
    ServiceReceiverPolicy::new(
        uri("https://issuer.example.invalid"),
        home(operation),
        vec![trust(operation)],
    )
    .unwrap()
}
fn facts(operation: ServiceOperation) -> ServiceTokenFacts {
    ServiceTokenFacts {
        issuer: uri("https://issuer.example.invalid"),
        audience: trust(operation).audience,
        scope: operation.scope().into(),
        issued_at: UnixSeconds(1_000),
        expires_at: UnixSeconds(1_060),
        caller: if is_directory_call(operation) {
            ServiceCallerIdentity::RegisteredClient {
                client_id: client("pinned-directory"),
                kind: RegisteredServiceClientKind::FleetDirectory,
            }
        } else {
            ServiceCallerIdentity::Issuer
        },
    }
}

// Positive controls independently enumerate all PD C7 scopes. The domain
// facts/signature validity are a stand-in for the still producer-owned K7
// adapter; these tests prove policy, not cryptographic authentication or SO7.
#[test]
fn every_c7_operation_has_the_exact_declared_scope_and_permitted_caller() {
    let scopes = [
        "admit-sign-in",
        "person-standing",
        "central-lookup",
        "device-login-decisions",
        "machine-token-decisions",
        "device-approval-authorize",
        "device-approval-activate",
        "device-approval-cancel",
        "initialize-organization",
    ];
    for (operation, expected_scope) in OPERATIONS.into_iter().zip(scopes) {
        assert_eq!(operation.scope(), expected_scope);
        let selected = policy(operation)
            .authorize(
                SignatureValidity::Valid,
                facts(operation),
                UnixSeconds(1_000),
            )
            .unwrap();
        assert_eq!(selected.operation(), operation);
    }
}

#[test]
fn an_audience_cannot_borrow_another_operations_scope() {
    for operation in OPERATIONS {
        let policy = policy(operation);
        for other in OPERATIONS.into_iter().filter(|other| *other != operation) {
            let mut candidate = facts(operation);
            candidate.scope = other.scope().into();
            assert!(matches!(
                policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
                Err(ServiceReceiverRefusal::WrongOperation)
            ));
            let mut candidate = facts(operation);
            candidate.audience = trust(other).audience;
            assert!(matches!(
                policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
                Err(ServiceReceiverRefusal::WrongOperation)
            ));
        }
    }
}

#[test]
fn each_audience_refuses_every_wrong_registered_client_kind() {
    for operation in OPERATIONS {
        let policy = policy(operation);
        for kind in KINDS {
            let mut candidate = facts(operation);
            candidate.caller = ServiceCallerIdentity::RegisteredClient {
                client_id: client("pinned-directory"),
                kind,
            };
            let answer = policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000));
            if is_directory_call(operation) && kind == RegisteredServiceClientKind::FleetDirectory {
                assert!(answer.is_ok());
            } else {
                assert!(matches!(answer, Err(ServiceReceiverRefusal::WrongCaller)));
            }
        }
        if is_directory_call(operation) {
            let mut candidate = facts(operation);
            candidate.caller = ServiceCallerIdentity::Issuer;
            assert!(matches!(
                policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
                Err(ServiceReceiverRefusal::WrongCaller)
            ));
            let mut candidate = facts(operation);
            candidate.caller = ServiceCallerIdentity::RegisteredClient {
                client_id: client("another-directory"),
                kind: RegisteredServiceClientKind::FleetDirectory,
            };
            assert!(matches!(
                policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
                Err(ServiceReceiverRefusal::WrongCaller)
            ));
        }
    }
}

#[test]
fn central_rejects_a_valid_call_naming_another_organization() {
    for operation in OPERATIONS {
        let selected = policy(operation)
            .authorize(
                SignatureValidity::Valid,
                facts(operation),
                UnixSeconds(1_000),
            )
            .unwrap();
        assert_eq!(
            selected.require_served_organization(&organization("01")),
            Ok(())
        );
        match home(operation) {
            ServiceReceiverHome::Central { .. } => assert_eq!(
                selected.require_served_organization(&organization("02")),
                Err(ServiceReceiverRefusal::NotThisCentral)
            ),
            ServiceReceiverHome::FleetDirectory => assert_eq!(
                selected.require_served_organization(&organization("02")),
                Ok(())
            ),
        }
    }
}

#[test]
fn operation_selects_the_nominal_lifetime_before_any_body_decoder() {
    for operation in OPERATIONS {
        let policy = policy(operation);
        let maximum = if is_directory_call(operation) {
            300
        } else {
            60
        };
        let mut candidate = facts(operation);
        candidate.expires_at = UnixSeconds(1_000 + maximum);
        assert!(
            policy
                .authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000))
                .is_ok()
        );
        let mut candidate = facts(operation);
        candidate.expires_at = UnixSeconds(1_001 + maximum);
        assert!(matches!(
            policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
            Err(ServiceReceiverRefusal::Time(
                ServiceTokenTimeRefusal::LifetimeExceeded
            ))
        ));
    }
}

#[test]
fn invalid_signature_or_unpinned_issuer_cannot_select_an_operation() {
    for operation in OPERATIONS {
        let policy = policy(operation);
        assert!(matches!(
            policy.authorize(
                SignatureValidity::Invalid,
                facts(operation),
                UnixSeconds(1_000)
            ),
            Err(ServiceReceiverRefusal::BadSignature)
        ));
        let mut candidate = facts(operation);
        candidate.issuer = uri("https://another-issuer.example.invalid");
        assert!(matches!(
            policy.authorize(SignatureValidity::Valid, candidate, UnixSeconds(1_000)),
            Err(ServiceReceiverRefusal::WrongIssuer)
        ));
    }
}

#[test]
fn configuration_cannot_share_an_audience_or_put_an_operation_at_the_wrong_home() {
    let issuer = uri("https://issuer.example.invalid");
    let central = home(ServiceOperation::PersonStanding);
    let mut admit = trust(ServiceOperation::AdmitSignIn);
    admit.audience = trust(ServiceOperation::PersonStanding).audience;
    assert!(matches!(
        ServiceReceiverPolicy::new(
            issuer.clone(),
            central.clone(),
            vec![trust(ServiceOperation::PersonStanding), admit]
        ),
        Err(ServiceReceiverRefusal::InvalidConfiguration)
    ));
    assert!(matches!(
        ServiceReceiverPolicy::new(
            issuer.clone(),
            central.clone(),
            vec![trust(ServiceOperation::CentralLookup)]
        ),
        Err(ServiceReceiverRefusal::InvalidConfiguration)
    ));
    let mut wrong_pin = trust(ServiceOperation::InitializeOrganization);
    wrong_pin.caller = PinnedServiceCaller::Issuer;
    assert!(matches!(
        ServiceReceiverPolicy::new(issuer, central, vec![wrong_pin]),
        Err(ServiceReceiverRefusal::InvalidConfiguration)
    ));
}

#[test]
fn a_pinned_client_identifier_is_nonempty_and_need_not_be_a_generated_machine_uuid() {
    assert!(ServiceClientId::try_from(String::new()).is_err());
    assert_eq!(client("pinned-directory").as_str(), "pinned-directory");
}

fn body_literal(operation: ServiceOperation) -> serde_json::Value {
    use serde_json::json;
    let organization = organization("01").as_str().to_owned();
    let person = "019f0000-0000-7000-8000-000000000002";
    let request = "019f0000-0000-7000-8000-000000000003";
    let decision = json!({"organizationId":organization,"approver":person,"requestId":request,
        "installation":"019f0000-0000-7000-8000-000000000004",
        "peer":"019f0000-0000-7000-8000-000000000005","label":"fixture machine","allowedWorkspaces":[]});
    match operation {
        ServiceOperation::AdmitSignIn => {
            json!({"operation":"admitSignIn","organizationId":organization,"personId":person,"verifiedEmail":"owner@example.invalid"})
        }
        ServiceOperation::PersonStanding => {
            json!({"operation":"standing","organizationId":organization,"personId":person})
        }
        ServiceOperation::CentralLookup => json!({"organizationId":organization}),
        ServiceOperation::DeviceLoginDecisions => decision,
        ServiceOperation::MachineTokenDecisions => json!({"organizationId":organization,
            "enrollmentId":"019f0000-0000-7000-8000-000000000006","clientId":"019f0000-0000-7000-8000-000000000007"}),
        ServiceOperation::DeviceApprovalAuthorize => json!({"decision":decision}),
        ServiceOperation::DeviceApprovalActivate | ServiceOperation::DeviceApprovalCancel => {
            json!({"organizationId":organization,"requestId":request})
        }
        ServiceOperation::InitializeOrganization => {
            json!({"organizationId":organization,"ownerEmail":"owner@example.invalid","commandId":"019f0000-0000-7000-8000-000000000008"})
        }
    }
}

fn decode_body(
    operation: ServiceOperation,
    body: &serde_json::Value,
) -> Result<collaboration_central_core::AuthorizedServiceRequest, ServiceReceiverRefusal> {
    policy(operation)
        .authorize(
            SignatureValidity::Valid,
            facts(operation),
            UnixSeconds(1_000),
        )?
        .decode_body(&serde_json::to_vec(body).map_err(|_| ServiceReceiverRefusal::WrongOperation)?)
}

// Real common DTOs stay at this boundary. K7 authentication remains a named
// stand-in, while the credential-selected decoder and organization guard run.
#[test]
fn each_credential_decodes_only_its_selected_body_and_never_falls_back() {
    use collaboration_central_core::ServiceRequestBody;
    for operation in OPERATIONS {
        let expected = body_literal(operation);
        let body = decode_body(operation, &expected).unwrap().into_body();
        assert!(matches!(
            (operation, body),
            (
                ServiceOperation::AdmitSignIn,
                ServiceRequestBody::AdmitSignIn(_)
            ) | (
                ServiceOperation::PersonStanding,
                ServiceRequestBody::PersonStanding(_)
            ) | (
                ServiceOperation::CentralLookup,
                ServiceRequestBody::CentralLookup(_)
            ) | (
                ServiceOperation::DeviceLoginDecisions,
                ServiceRequestBody::DeviceLoginDecisions(_)
            ) | (
                ServiceOperation::MachineTokenDecisions,
                ServiceRequestBody::MachineTokenDecisions(_)
            ) | (
                ServiceOperation::DeviceApprovalAuthorize,
                ServiceRequestBody::DeviceApprovalAuthorize(_)
            ) | (
                ServiceOperation::DeviceApprovalActivate,
                ServiceRequestBody::DeviceApprovalActivate(_)
            ) | (
                ServiceOperation::DeviceApprovalCancel,
                ServiceRequestBody::DeviceApprovalCancel(_)
            ) | (
                ServiceOperation::InitializeOrganization,
                ServiceRequestBody::InitializeOrganization(_)
            )
        ));
        for other in OPERATIONS {
            let candidate = body_literal(other);
            // Activate and cancel intentionally share SettleDeviceApproval.
            // Their distinct credential fixes which action the handler sees.
            if candidate == expected {
                continue;
            }
            assert!(
                matches!(
                    decode_body(operation, &candidate),
                    Err(ServiceReceiverRefusal::WrongOperation)
                ),
                "{operation:?} accepted {other:?}'s different body"
            );
        }
        let mut extra = expected;
        extra
            .as_object_mut()
            .unwrap()
            .insert("operationOverride".into(), serde_json::json!("cancel"));
        assert!(matches!(
            decode_body(operation, &extra),
            Err(ServiceReceiverRefusal::WrongOperation)
        ));
    }
}

#[test]
fn the_selected_body_checks_organization_before_becoming_a_handler_request() {
    for operation in OPERATIONS {
        let mut body = body_literal(operation);
        if operation == ServiceOperation::DeviceApprovalAuthorize {
            body["decision"]["organizationId"] = serde_json::json!(organization("02").as_str());
        } else {
            body["organizationId"] = serde_json::json!(organization("02").as_str());
        }
        let result = decode_body(operation, &body);
        match home(operation) {
            ServiceReceiverHome::Central { .. } => assert!(matches!(
                result,
                Err(ServiceReceiverRefusal::NotThisCentral)
            )),
            ServiceReceiverHome::FleetDirectory => assert!(result.is_ok()),
        }
    }
}
