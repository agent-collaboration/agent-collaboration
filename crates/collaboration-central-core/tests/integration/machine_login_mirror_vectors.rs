//! Exact issuer-owned vectors from switchboard b1c9f6f6, with no local verdict changes.
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
fn judge<T: DeserializeOwned + Serialize>(vector: &Vector) -> Result<(), String> {
    let parsed = serde_json::from_value::<T>(vector.input().clone());
    if matches!(vector, Vector::Refuse { .. }) {
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
fn check(vector: &Vector) -> Result<(), String> {
    match vector.contract() {
        "DecideDeviceLogin" => judge::<DecideDeviceLogin>(vector),
        "DeviceLoginDecision" => judge::<DeviceLoginDecision>(vector),
        "DeviceLoginRefusal" => judge::<DeviceLoginRefusal>(vector),
        "DecideMachineToken" => judge::<DecideMachineToken>(vector),
        "MachineTokenDecision" => judge::<MachineTokenDecision>(vector),
        "MachineTokenRefusal" => judge::<MachineTokenRefusal>(vector),
        "OrganizationChangeRecord" => judge::<OrganizationChangeRecord>(vector),
        other => Err(format!("unmapped central contract: {other}")),
    }
}

#[test]
fn central_agrees_with_every_issuer_vector_byte_for_byte() {
    assert_eq!(
        format!("{:x}", Sha256::digest(VECTORS.as_bytes())),
        "e1aef1e60ec377e284cc27b64bd326788f979f95916c16d2251a0f092972dbf8"
    );
    let file: VectorFile = serde_json::from_str(VECTORS).unwrap();
    assert!(file.source.contains("N03"));
    assert_eq!(file.vectors.len(), 106);
    let mut disagreements = Vec::new();
    for (index, vector) in file.vectors.iter().enumerate() {
        if let Err(disagreement) = check(vector) {
            disagreements.push(format!(
                "vector {} {}: {disagreement}",
                index + 1,
                vector.contract()
            ));
        }
    }
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
fn every_issuer_decision_variant_is_consumed_without_local_exceptions() {
    assert_eq!(
        format!("{:x}", Sha256::digest(VARIANTS.as_bytes())),
        "111526de90e8d1766cd88971b0f4fbb1f211678428cde7daaca053ddc9d795d0"
    );
    let variants: DecisionVariants = serde_json::from_str(VARIANTS).unwrap();
    assert_eq!(variants.device_login.len(), 5);
    assert_eq!(variants.machine_token.len(), 6);
    for input in variants.device_login {
        let parsed: DeviceLoginDecision = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), input);
    }
    for input in variants.machine_token {
        let parsed: MachineTokenDecision = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), input);
    }
}
