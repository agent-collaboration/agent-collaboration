//! IDs and timestamps use the same clock on a real, controlled native object.

use super::{NativeObjects, ObjectAddress, StoreMigrations};
use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest};
use collaboration_central_core::RegistryTime;
use fleet_sync_lease::RegistryConfig;
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::Arc;
use std::time::Duration;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_700_000_000_000)
    }
}

#[tokio::test]
async fn object_ids_follow_held_and_advanced_conformance_time() {
    let root = tempfile::tempdir().unwrap();
    let objects = NativeObjects::open_with_controls(
        root.path(),
        Arc::new(FixedClock),
        RegistryConfig {
            lease: fleet_sync_lease::LeasePolicy {
                lease_duration: Duration::from_secs(14_400),
                renew_interval: Duration::from_secs(900),
            },
            authoritative_receiver: "019f0000-0000-7000-8000-000000000010"
                .to_owned()
                .try_into()
                .unwrap(),
            release_unsent_for: Duration::from_secs(300),
            move_needs_renewal_within: None,
        },
        crate::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000000011"
                .to_owned()
                .try_into()
                .unwrap(),
        },
        StoreMigrations::production(),
        Duration::from_secs(30),
        Duration::from_secs(30),
        Vec::new(),
    )
    .await
    .unwrap();
    let slot = objects.slot(ObjectAddress::Fleet).await.unwrap();
    for held_ms in [4_102_444_800_000, 4_102_444_800_123] {
        objects
            .runtime_control(RuntimeControlRequest {
                address: ObjectAddress::Fleet,
                command: RuntimeControl::SetTime {
                    now: RegistryTime(chrono::DateTime::from_timestamp_millis(held_ms).unwrap()),
                },
            })
            .await
            .unwrap();
        assert_eq!(slot.io.now().unix_millis(), held_ms);
        let ids = [slot.io.new_id(), slot.io.new_id()];
        for id in ids {
            let (seconds, nanos) = id.get_timestamp().unwrap().to_unix();
            let id_ms = seconds * 1_000 + u64::from(nanos) / 1_000_000;
            assert_eq!(
                id_ms,
                u64::try_from(held_ms).unwrap(),
                "object id bypassed its effective clock"
            );
        }
        assert!(ids[0] < ids[1]);
    }
    objects.shutdown().await.unwrap();
}
