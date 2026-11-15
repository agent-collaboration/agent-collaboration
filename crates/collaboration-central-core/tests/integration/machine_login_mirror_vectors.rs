//! Exact issuer997 vectors, with only the approved former-service hard cut.
//! The source fixture is copied byte-for-byte; no null-output rewrite remains.
use collaboration_central_core::identity_contract::{
    DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision, DeviceLoginRefusal,
    MachineTokenDecision, MachineTokenRefusal,
};
use collaboration_central_core::organization_authority::OrganizationChangeRecord;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use sha2::{Digest, Sha256};

const VECTORS: &str =
    include_str!("../../contracts/issuer-mirror/machine-login-validation-vectors.json");
const VARIANTS: &str =
    include_str!("../../contracts/issuer-mirror/machine-login-decision-variants.json");
// One-based ordinals in the immutable121-vector source. These cases are tested
// as refusals, including the four issuer accepts; none is skipped.
const REMOVED_FORMER_SERVICE: [usize; 18] = [
    50, 51, 52, 53, 54, 55, 56, 57, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89,
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorFile {
    source: String,
    vectors: Vec<Vector>,
}
#[derive(Deserialize)]
#[serde(tag = "verdict", rename_all = "camelCase", deny_unknown_fields)]
enum Vector {
    Accept {
        contract: String,
        input: Value,
        output: String,
    },
    Refuse {
        contract: String,
        input: Value,
    },
}
impl Vector {
    fn contract(&self) -> &str {
        match self {
            Self::Accept { contract, .. } | Self::Refuse { contract, .. } => contract,
        }
    }
    fn input(&self) -> &Value {
        match self {
            Self::Accept { input, .. } | Self::Refuse { input, .. } => input,
        }
    }
}
fn former_service_clause(contract: &str, input: &Value) -> bool {
    match contract {
        "DecideDeviceLogin" => input.get("formerServiceId").is_some(),
        "DeviceLoginDecision" => {
            input.pointer("/reason/kind").and_then(Value::as_str)
                == Some("formerServiceLinkedElsewhere")
        }
        "DeviceLoginRefusal" => {
            input.get("kind").and_then(Value::as_str) == Some("formerServiceLinkedElsewhere")
        }
        _ => false,
    }
}
fn judge<T: DeserializeOwned + Serialize>(vector: &Vector, removed: bool) -> Result<(), String> {
    let parsed = serde_json::from_value::<T>(vector.input().clone());
    if removed || matches!(vector, Vector::Refuse { .. }) {
        return if parsed.is_err() {
            Ok(())
        } else {
            Err("accepted a refused input".to_owned())
        };
    }
    let Vector::Accept { output, .. } = vector else {
        return Err("refusal vector reached the accepted-output comparison".to_owned());
    };
    let parsed = parsed.map_err(|error| format!("refused an accepted input: {error}"))?;
    let written = serde_json::to_string(&parsed).map_err(|error| error.to_string())?;
    if &written == output {
        Ok(())
    } else {
        Err("serialized bytes differ from issuer output".to_owned())
    }
}
fn check(vector: &Vector, removed: bool) -> Result<(), String> {
    match vector.contract() {
        "DecideDeviceLogin" => judge::<DecideDeviceLogin>(vector, removed),
        "DeviceLoginDecision" => judge::<DeviceLoginDecision>(vector, removed),
        "DeviceLoginRefusal" => judge::<DeviceLoginRefusal>(vector, removed),
        "DecideMachineToken" => judge::<DecideMachineToken>(vector, removed),
        "MachineTokenDecision" => judge::<MachineTokenDecision>(vector, removed),
        "MachineTokenRefusal" => judge::<MachineTokenRefusal>(vector, removed),
        "OrganizationChangeRecord" => judge::<OrganizationChangeRecord>(vector, removed),
        other => Err(format!("unmapped central contract: {other}")),
    }
}

#[test]
fn central_agrees_with_every_issuer_vector_byte_for_byte_except_removed_former_service_clauses() {
    assert_eq!(
        format!("{:x}", Sha256::digest(VECTORS.as_bytes())),
        "c567cbb2ebcf75cbf6392e2990abebd8f988491c81483bf67cd09ac919ee8253"
    );
    let file: VectorFile = serde_json::from_str(VECTORS).unwrap();
    assert!(file.source.contains("N03"));
    assert_eq!(file.vectors.len(), 121);
    let mut removed_count = 0;
    let mut changed_verdicts = 0;
    let mut disagreements = Vec::new();
    for (index, vector) in file.vectors.iter().enumerate() {
        let ordinal = index + 1;
        let removed = REMOVED_FORMER_SERVICE.contains(&ordinal);
        assert_eq!(
            removed,
            former_service_clause(vector.contract(), vector.input()),
            "unreviewed disposition at vector {ordinal}"
        );
        if removed {
            removed_count += 1;
            if matches!(vector, Vector::Accept { .. }) {
                changed_verdicts += 1;
            }
        }
        if let Err(disagreement) = check(vector, removed) {
            disagreements.push(format!(
                "vector {ordinal} {}: {disagreement}",
                vector.contract()
            ));
        }
    }
    assert_eq!(removed_count, 18);
    assert_eq!(changed_verdicts, 4);
    assert!(
        disagreements.is_empty(),
        "issuer vector disagreements:\n{}",
        disagreements.join("\n")
    );
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecisionVariants {
    device_login: Vec<Value>,
    machine_token: Vec<Value>,
}

#[test]
fn every_issuer_decision_variant_is_consumed_with_only_the_removed_refusal_rejected() {
    assert_eq!(
        format!("{:x}", Sha256::digest(VARIANTS.as_bytes())),
        "80757bdb1dc1edb604675185a045e39deca76c8291e2790b16e00be4a9ebba67"
    );
    let variants: DecisionVariants = serde_json::from_str(VARIANTS).unwrap();
    assert_eq!(variants.device_login.len(), 6);
    assert_eq!(variants.machine_token.len(), 6);
    for (index, input) in variants.device_login.into_iter().enumerate() {
        let removed = index == 5;
        assert_eq!(
            removed,
            former_service_clause("DeviceLoginDecision", &input)
        );
        let parsed = serde_json::from_value::<DeviceLoginDecision>(input.clone());
        if removed {
            assert!(parsed.is_err());
        } else {
            assert_eq!(serde_json::to_value(parsed.unwrap()).unwrap(), input);
        }
    }
    for input in variants.machine_token {
        let parsed: MachineTokenDecision = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), input);
    }
}
