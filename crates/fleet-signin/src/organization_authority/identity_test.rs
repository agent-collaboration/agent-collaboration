use super::{OrganizationIdentity, OrganizationIdentityRefusal, ProvisioningStampAction};
use fleet_identity_protocol::OrganizationId;

fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}

// Oracle: B2 data/flows: initialize once, preserve a repeat, never overwrite another org.
// No pre-fix semantic red has run for this new rule; database serialization is separate proof.
#[test]
fn provisioning_distinguishes_initialization_repeat_and_foreign_organization() {
    let first = organization("019a0000-0000-7000-8000-000000000001");
    let second = organization("019a0000-0000-7000-8000-000000000002");
    let unstamped = OrganizationIdentity::Unprovisioned;
    let stamped = OrganizationIdentity::Provisioned {
        organization_id: first.clone(),
    };

    assert_eq!(
        unstamped.provisioning_action(&first),
        Ok(ProvisioningStampAction::Initialize)
    );
    assert_eq!(
        stamped.provisioning_action(&first),
        Ok(ProvisioningStampAction::AlreadyProvisioned)
    );
    assert_eq!(
        stamped.provisioning_action(&second),
        Err(OrganizationIdentityRefusal::OrganizationMismatch)
    );
}

// Catch a default organization, an unstamped public read, or an ID-insensitive guard.
#[test]
fn public_requests_require_a_stamp_for_the_same_organization() {
    let first = organization("019a0000-0000-7000-8000-000000000001");
    let second = organization("019a0000-0000-7000-8000-000000000002");
    let unstamped = OrganizationIdentity::Unprovisioned;
    let stamped = OrganizationIdentity::Provisioned {
        organization_id: first.clone(),
    };

    assert_eq!(
        unstamped.require_matching_organization(&first),
        Err(OrganizationIdentityRefusal::Unprovisioned)
    );
    assert_eq!(stamped.require_matching_organization(&first), Ok(()));
    assert_eq!(
        stamped.require_matching_organization(&second),
        Err(OrganizationIdentityRefusal::OrganizationMismatch)
    );
}
