//! Signed machine bearer inputs shared by the real native and Worker fixtures.
use super::{ConformanceResult, ISSUER, json, sign};

pub const MACHINE_AUDIENCE: &str = "https://central.example.invalid/machine";

pub fn machine_token(
    organization: &str,
    installation: &str,
    enrollment: &str,
    now_seconds: i64,
    grant_expires_at: i64,
) -> ConformanceResult<String> {
    sign(
        &json!({"alg":"EdDSA","typ":"at+jwt","kid":"stamp-a"}),
        &json!({"iss":ISSUER,"aud":MACHINE_AUDIENCE,"iat":now_seconds,
            "exp":now_seconds+3600,"client_id":enrollment,"azp":enrollment,
            "organization_id":organization,"grant_expires_at":grant_expires_at,
            "principal":{"kind":"machine","installation_id":installation,"enrollment_id":enrollment}}),
        7,
    )
}
