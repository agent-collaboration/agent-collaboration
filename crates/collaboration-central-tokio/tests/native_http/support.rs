use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::ReceiverId;
use std::time::Duration;

pub type TestError = Box<dyn std::error::Error + Send + Sync>;

pub fn registry_config() -> Result<RegistryConfig, TestError> {
    Ok(RegistryConfig {
        lease: LeasePolicy {
            lease_duration: Duration::from_secs(14_400),
            renew_interval: Duration::from_secs(900),
        },
        authoritative_receiver: ReceiverId::try_from(
            "019f0000-0000-7000-8000-000000000010".to_owned(),
        )?,
        // Explicit fixture value; production policy is not chosen by this test.
        release_unsent_for: Duration::ZERO,
        move_needs_renewal_within: Some(Duration::from_secs(1800)),
    })
}
