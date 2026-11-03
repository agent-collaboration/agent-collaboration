//! Controlled person-proof inputs; these are not real B4a producer outputs.

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::organization_authority::{
    ParsedPersonToken, PersonTokenPolicy, PersonTokenRefusal,
};
use fleet_identity_protocol::UnixMillis;
use serde_json::{Value, json};

pub const ISSUER: &str = "https://signin.example.invalid";
pub const ADMIN: &str = "https://central.example.invalid/v1/admin";
pub const OPERATOR: &str = "https://central.example.invalid/v1/operator";
pub const PERSON: &str = "019a3dce-2000-7000-8000-000000000001";
pub const ORGANIZATION: &str = "019a3dce-2000-7000-8000-000000000002";
pub const SIGN_IN_SESSION: &str = "019a3dce-2000-7000-8000-000000000003";
pub const NOW: UnixMillis = UnixMillis(1_800_000_000_000);

pub fn claims() -> Value {
    json!({
        "iss": ISSUER, "sub": PERSON, "aud": ADMIN,
        "exp": 1_800_000_300, "iat": 1_800_000_000,
        "scope": "central:admin", "client_id": "sign-in-app",
        "azp": "sign-in-app", "sid": SIGN_IN_SESSION,
        "jti": "issuer-owned-token", "organization_id": ORGANIZATION,
        "second_factor_at": 1_799_999_950, "amr": ["otp"],
    })
}

pub fn policy() -> Result<PersonTokenPolicy, PersonTokenRefusal> {
    PersonTokenPolicy::new(
        ISSUER.into(),
        ADMIN.into(),
        OPERATOR.into(),
        "sign-in-app".into(),
    )
}

pub fn token(header: &Value, body: &Value) -> Result<String, serde_json::Error> {
    Ok(format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(body)?),
        URL_SAFE_NO_PAD.encode([1_u8, 2, 3]),
    ))
}

pub fn person_token(
    body: &Value,
) -> Result<ParsedPersonToken, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ParsedPersonToken::parse(token(
        &json!({"typ":"at+jwt", "alg":"EdDSA", "kid":"issuer-key"}),
        body,
    )?)?)
}
