//! Cross-repo equality with the issuer's machine-login contracts.
//!
//! `contracts/issuer-mirror/machine-login-validation-vectors.json` is a byte-exact
//! copy of switchboard's
//! `packages/sign-in-core/contract-fixtures/machine-login/validation-vectors.json`
//! at commit de59e3c4b92d4d8c76524cda92cbf93347070dcf (SHA-256
//! f3d317be0268ecf0efa289b46d181dc3fb455847faddcdfc88fe46ec1fdcc847). Each vector
//! is an input the issuer's Zod schema accepts or refuses. For an accepted one,
//! `output` is the issuer's exact serialization of what it parsed.
//!
//! Central must refuse exactly the refused inputs and accept exactly the accepted
//! ones, and must serialize each accepted value to the same bytes. One named
//! exception (the Lead's ruling on row 6, PD:571): the issuer writes
//! `"approverSecondFactorAt":null`, while central omits an absent value.
use collaboration_central_core::identity_contract::{
    DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision, DeviceLoginRefusal,
    MachineTokenDecision, MachineTokenRefusal,
};
use collaboration_central_core::organization_authority::OrganizationChangeRecord;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

const VECTORS: &str =
    include_str!("../../contracts/issuer-mirror/machine-login-validation-vectors.json");
const NULL_SECOND_FACTOR: &str = r#","approverSecondFactorAt":null"#;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorFile {
    #[expect(dead_code, reason = "names the issuer source; the doc comment pins it")]
    source: String,
    vectors: Vec<Vector>,
}

#[derive(serde::Deserialize)]
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

/// What central did with one vector, or why it disagrees with the issuer.
fn judge<T: DeserializeOwned + Serialize>(vector: &Vector) -> Result<(), String> {
    match vector {
        Vector::Accept { input, output, .. } => {
            let parsed: T = serde_json::from_value(input.clone())
                .map_err(|error| format!("refused an accepted input ({error}): {input}"))?;
            let written = serde_json::to_string(&parsed).map_err(|error| error.to_string())?;
            let expected = if output.matches(NULL_SECOND_FACTOR).count() == 1 {
                output.replacen(NULL_SECOND_FACTOR, "", 1)
            } else {
                output.clone()
            };
            if written == expected {
                Ok(())
            } else {
                Err(format!("wrote {written}, issuer wrote {output}"))
            }
        }
        Vector::Refuse { input, .. } => match serde_json::from_value::<T>(input.clone()) {
            Ok(_) => Err(format!("accepted a refused input: {input}")),
            Err(_) => Ok(()),
        },
    }
}

#[test]
fn central_agrees_with_every_issuer_vector_byte_for_byte() {
    let file: VectorFile = serde_json::from_str(VECTORS).unwrap();
    assert!(!file.vectors.is_empty(), "the vector file holds no vectors");
    let mut disagreements = Vec::new();
    for vector in &file.vectors {
        let (Vector::Accept { contract, .. } | Vector::Refuse { contract, .. }) = vector;
        let verdict = match contract.as_str() {
            "DecideDeviceLogin" => judge::<DecideDeviceLogin>(vector),
            "DeviceLoginDecision" => judge::<DeviceLoginDecision>(vector),
            "DeviceLoginRefusal" => judge::<DeviceLoginRefusal>(vector),
            "DecideMachineToken" => judge::<DecideMachineToken>(vector),
            "MachineTokenDecision" => judge::<MachineTokenDecision>(vector),
            "MachineTokenRefusal" => judge::<MachineTokenRefusal>(vector),
            "OrganizationChangeRecord" => judge::<OrganizationChangeRecord>(vector),
            other => Err(format!("names no central contract: {other}")),
        };
        if let Err(disagreement) = verdict {
            disagreements.push(format!("{contract}: {disagreement}"));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} of {} vectors disagree:\n{}",
        disagreements.len(),
        file.vectors.len(),
        disagreements.join("\n")
    );
}
