//! Registry settlements retain their receipt beside the committed lease projection.
use super::support::{Fixture, TestResult};
use collaboration_central_core::RegistryIncarnation;
use fleet_sync_lease::{DatabaseLifecycle, RegistryGrant, RegistryRevision, RegistrySnapshot};
use fleet_sync_protocol::{DatabaseId, Epoch, Origin, PeerId};
use message_board::ServiceId;
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use std::num::NonZeroU64;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
pub(super) fn encoded_revision(
    incarnation: RegistryIncarnation,
    epoch: u64,
    change: u64,
    service: &str,
    peer: &str,
    changed_at_ms: i64,
) -> TestResult<Value> {
    let snapshot = RegistrySnapshot {
        database: DatabaseId::new(durable_steps::Uuid::parse_str(PROJECT)?)?,
        revision: RegistryRevision {
            epoch: Epoch::new(epoch)?,
            change: NonZeroU64::new(change).ok_or("positive fixture revision")?,
        },
        changed_at: SyncTime::from_unix_millis(changed_at_ms),
        lifecycle: DatabaseLifecycle::Registered {
            origin: Origin::Empty,
            imported_from: None,
        },
        grant: Some(RegistryGrant {
            epoch: Epoch::new(epoch)?,
            holder: PeerId::new(durable_steps::Uuid::parse_str(peer)?)?,
            expires_at: SyncTime::from_unix_millis(
                changed_at_ms
                    .checked_add(14_400_000)
                    .ok_or("fixture deadline overflow")?,
            ),
        }),
    };
    let holder = Some(ServiceId::try_from(service.to_owned())?);
    // The producer uses ring for the declared snapshot/content digest; the receiver uses sha2.
    let bytes = serde_json::to_vec(&(&snapshot, &holder))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(json!({"snapshot":snapshot,"incarnation":incarnation,"holder":holder,"digest":digest}))
}

pub(super) fn registry_call(
    kind: &str,
    request: Value,
    operation: durable_steps::Uuid,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":operation,"step":"claim_settlement"},
        "kind":kind,"target":null,"request":request
    }))?)
}

pub(super) async fn acquire_incarnation(fixture: &Fixture) -> TestResult<RegistryIncarnation> {
    let answer = fixture
        .runtime
        .on_call(registry_call(
            "central.lease-incarnation",
            json!({"atLeast":0}),
            durable_steps::Uuid::now_v7(),
        )?)
        .await
        .map_err(|error| format!("Registry incarnation: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}
