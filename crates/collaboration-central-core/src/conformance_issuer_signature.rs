//! A clock-free observation of the actual host capability, excluded from production.
use crate::{CentralPlatform, SignatureValidity};
use serde::{Deserialize, Serialize};

pub const ISSUER_SIGNATURE_PROBE_PATH: &str = "/__conformance/issuer-signature";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IssuerSignatureProbe {
    pub public_key: Vec<u8>,
    pub signed: Vec<u8>,
    pub signature: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "camelCase", deny_unknown_fields)]
pub enum IssuerSignatureObservation {
    Valid {},
    Invalid {},
    Crypto {},
}

pub async fn observe_issuer_signature<Platform: CentralPlatform>(
    platform: &Platform,
    input: IssuerSignatureProbe,
) -> IssuerSignatureObservation {
    match platform
        .verify_ed25519(&input.public_key, &input.signed, &input.signature)
        .await
    {
        Ok(SignatureValidity::Valid) => IssuerSignatureObservation::Valid {},
        Ok(SignatureValidity::Invalid) => IssuerSignatureObservation::Invalid {},
        Err(_) => IssuerSignatureObservation::Crypto {},
    }
}
