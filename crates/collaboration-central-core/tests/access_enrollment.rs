#![allow(clippy::expect_used)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
#[test]
fn access_client_id_and_machine_credential_bind_to_the_same_enrollment() {
    let mut harness = Harness::new();
    harness.enroll(1);
    harness.enroll(2);
    let authenticate = |client: &str| {
        WireCall::Registry(RegistryCall::AuthenticateMachine {
            credential_hash: "hash-1".into(),
            access_client_id: AccessClientId::new(client).expect("client"),
        })
    };
    assert!(
        matches!(harness.call("registry",authenticate("client-1")),WireResult::Registry(RegistryResult::Authenticated(CredentialStatus::Machine { installation,.. })) if installation==machine(1))
    );
    assert_eq!(
        harness.call("registry", authenticate("client-2")),
        WireResult::Registry(RegistryResult::Authenticated(CredentialStatus::Rejected(
            CredentialRejection::NotMachine
        )))
    );
    harness.call(
        "registry",
        admin(AdminRequest::RevokeMachine {
            installation: machine(1),
        }),
    );
    assert_eq!(
        harness.call("registry", authenticate("client-1")),
        WireResult::Registry(RegistryResult::Authenticated(CredentialStatus::Rejected(
            CredentialRejection::Revoked
        )))
    );
    harness.enroll(1);
    assert!(
        matches!(harness.call("registry",authenticate("client-1")),WireResult::Registry(RegistryResult::Authenticated(CredentialStatus::Machine { installation,.. })) if installation==machine(1))
    );
}
