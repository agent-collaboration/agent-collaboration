//! Registry settlements retain their receipt beside the committed lease projection.
use super::support::{Fixture, TestResult};
use collaboration_central_core::RegistryIncarnation;
use serde_json::{Value, json};

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
