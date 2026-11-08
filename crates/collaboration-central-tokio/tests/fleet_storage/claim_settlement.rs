//! Registry settlements retain their receipt beside the committed lease projection.
use super::support::TestResult;
use serde_json::{Value, json};

pub(super) fn registry_call(
    kind: &str,
    request: Value,
    operation: durable_steps::Uuid,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "organization":"019f0000-0000-7000-8000-00000000ac01","source":{"kind":"fleet","value":{"organization":"019f0000-0000-7000-8000-00000000ac01"}},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet","value":{"organization":"019f0000-0000-7000-8000-00000000ac01"}},"destinationFace":"fleet",
        "identity":{"operation":operation,"step":"claim_settlement"},
        "kind":kind,"target":null,"request":request
    }))?)
}
