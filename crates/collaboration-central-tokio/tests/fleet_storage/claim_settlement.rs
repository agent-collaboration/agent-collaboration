//! Registry settlements retain their receipt beside the committed lease projection.
use super::support::TestResult;
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
