use collaboration_protocol::{ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess};
use fleet_identity_protocol::{AbsolutePath, RemoteLaunchPolicy};
use proptest::prelude::*;
use serde_json::json;

#[test]
fn every_router_policy_preserves_the_exact_fleet_wire_in_both_directions() {
    for (access, wire) in [
        (RouterAccess::WriteRestricted, "write-restricted"),
        (RouterAccess::WorkspaceWrite, "workspace-write"),
    ] {
        let router = ProviderRequestedPolicy { access };
        let fleet = RemoteLaunchPolicy::from(router.clone());
        assert_eq!(
            serde_json::to_value(&router).unwrap(),
            json!({"access":wire})
        );
        assert_eq!(
            serde_json::to_value(&fleet).unwrap(),
            json!({"access":wire})
        );
        assert_eq!(ProviderRequestedPolicy::from(fleet), router);
    }
}

#[test]
fn working_directories_share_absolute_utf8_byte_boundaries() {
    for input in [
        "/".to_owned(),
        "//x".to_owned(),
        format!("/{}", "a".repeat(4095)),
        format!("/{}aaa", "😀".repeat(1023)),
    ] {
        assert!(ProviderWorkingDirectory::try_from(input.clone()).is_ok());
        assert!(AbsolutePath::try_from(input).is_ok());
    }
    for input in [
        "".to_owned(),
        "relative".to_owned(),
        "C:\\x".to_owned(),
        "/nul\0".to_owned(),
        format!("/{}aaaa", "😀".repeat(1023)),
    ] {
        assert!(ProviderWorkingDirectory::try_from(input.clone()).is_err());
        assert!(AbsolutePath::try_from(input).is_err());
    }
}

proptest! {
    #[test]
    fn working_directory_acceptance_agrees(input in any::<String>()) {
        prop_assert_eq!(ProviderWorkingDirectory::try_from(input.clone()).is_ok(), AbsolutePath::try_from(input).is_ok());
    }
}
