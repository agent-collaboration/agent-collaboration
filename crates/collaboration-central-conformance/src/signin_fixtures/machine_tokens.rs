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

/// Enroll through the actual private listener so socket tests can authenticate
/// an independent recipient in the same organization.
pub async fn enroll_another_machine(
    private: &reqwest::Client,
    origin: &str,
    organization: &str,
    machine_id: &str,
    request_id: &str,
    now: i64,
) -> ConformanceResult<String> {
    let approval = private
        .post(format!("{origin}/internal/device-login-decisions"))
        .json(
            &json!({"organizationId":organization,"approver":super::OWNER,
            "permissionChecked":"machine:enroll","requestId":request_id,
            "machineId":machine_id,"label":"Other recipient",
            "allowedWorkspaces":[],"approverSecondFactorAt":now-60,"codeExpiresAt":now+600}),
        )
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    let enrollment = approval
        .get("enrollmentId")
        .and_then(Value::as_str)
        .filter(|_| approval.get("result").and_then(Value::as_str) == Some("approved"))
        .ok_or("other machine approval refused")?;
    let grant = private
        .post(format!("{origin}/internal/machine-token-decisions"))
        .json(
            &json!({"organizationId":organization,"enrollmentId":enrollment,"clientId":enrollment}),
        )
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;
    if grant.get("result").and_then(Value::as_str) != Some("grant")
        || grant.get("machineId").and_then(Value::as_str) != Some(machine_id)
        || grant.get("enrollmentId").and_then(Value::as_str) != Some(enrollment)
    {
        return Err("other machine did not receive its own token grant".into());
    }
    let until = grant
        .get("grantExpiresAt")
        .and_then(Value::as_i64)
        .ok_or("other machine grant expiry")?;
    machine_token(organization, machine_id, enrollment, now, until)
}
