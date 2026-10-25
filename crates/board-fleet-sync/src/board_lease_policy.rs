//! Board lease duration and ordinary renewal cadence (U38).
use fleet_sync_lease::LeasePolicy;
use std::time::Duration;

pub const BOARD_LEASE_POLICY: LeasePolicy = LeasePolicy {
    lease_duration: Duration::from_secs(14_400),
    renew_interval: Duration::from_secs(900),
};
