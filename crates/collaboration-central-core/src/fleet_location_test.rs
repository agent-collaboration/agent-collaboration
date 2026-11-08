use super::{FleetLocationPolicy, FleetLocationRefusal};
use fleet_identity_protocol::OrganizationId;

const LEGACY: &str = "019f0000-0000-7000-8000-00000000ac01";
const FIRST: &str = "019f0000-0000-7000-8000-00000000ac02";
const SECOND: &str = "019f0000-0000-7000-8000-00000000ac03";
fn organization(value: &str) -> OrganizationId {
    value.to_owned().try_into().unwrap()
}

#[test]
fn fleet_location_cloudflare_names_keep_organizations_separate() {
    let policy = FleetLocationPolicy::new(organization(LEGACY));
    assert_eq!(policy.cloudflare_name(&organization(FIRST)), FIRST);
    assert_eq!(policy.cloudflare_name(&organization(SECOND)), SECOND);
    assert_ne!(
        policy.cloudflare_name(&organization(FIRST)),
        policy.cloudflare_name(&organization(SECOND))
    );
}

#[test]
fn fleet_location_only_the_configured_organization_uses_legacy_physical_names() {
    let policy = FleetLocationPolicy::new(organization(LEGACY));
    assert_eq!(policy.cloudflare_name(&organization(LEGACY)), "fleet");
    assert_eq!(policy.native_key(&organization(LEGACY)), "fleet");
    assert_eq!(
        policy.native_key(&organization(FIRST)),
        format!("fleet-{FIRST}")
    );
    assert_ne!(policy.cloudflare_name(&organization(FIRST)), "fleet");
    assert_eq!(
        policy.organization_from_native_key("fleet").unwrap(),
        organization(LEGACY)
    );
}

#[test]
fn fleet_location_native_reopen_recovers_the_same_nonlegacy_organization() {
    let policy = FleetLocationPolicy::new(organization(LEGACY));
    let key = policy.native_key(&organization(FIRST));
    assert_eq!(
        policy.organization_from_native_key(&key).unwrap(),
        organization(FIRST)
    );
    assert_eq!(
        policy.native_key(&organization(SECOND)),
        format!("fleet-{SECOND}")
    );
    assert_ne!(key, policy.native_key(&organization(SECOND)));
}

#[test]
fn fleet_location_rejects_aliases_and_unvalidated_file_keys() {
    let policy = FleetLocationPolicy::new(organization(LEGACY));
    for key in [
        "",
        "Fleet",
        "fleet-../fleet",
        "fleet-019f0000-0000-4000-8000-00000000ac02",
    ] {
        assert_eq!(
            policy.organization_from_native_key(key),
            Err(FleetLocationRefusal::InvalidPhysicalKey)
        );
    }
    assert_eq!(
        policy.organization_from_native_key(&format!("fleet-{LEGACY}")),
        Err(FleetLocationRefusal::InvalidPhysicalKey)
    );
}
