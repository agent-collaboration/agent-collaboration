//! Signed machine bearer inputs shared by the real native and Worker fixtures.
use super::{ConformanceResult, ISSUER, json, sign};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

pub const MACHINE_AUDIENCE: &str = "https://central.example.invalid/machine";

/// Change signed test claims independently of headers, signature and issuer.
pub fn resign_machine_token(token: &str, changes: &[(&str, Value)]) -> ConformanceResult<String> {
    let payload = token.split('.').nth(1).ok_or("machine token payload")?;
    let mut claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?;
    for (claim, value) in changes {
        super::set_claim(&mut claims, claim, value.clone())?;
    }
    sign(
        &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
        &claims,
        7,
    )
}

pub fn machine_token(
    organization: &str,
    machine_id: &str,
    enrollment: &str,
    now_seconds: i64,
    grant_expires_at: i64,
) -> ConformanceResult<String> {
    sign(
        &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
        &json!({"iss":ISSUER,"aud":MACHINE_AUDIENCE,"iat":now_seconds,
            "exp":now_seconds+600,"client_id":enrollment,"azp":enrollment,
            "organization_id":organization,"grant_expires_at":grant_expires_at,
            "principal":{"kind":"machine","machine_id":machine_id,"enrollment_id":enrollment}}),
        7,
    )
}
