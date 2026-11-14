//! Historical closed refusal vectors, signed with real fixture keys.
use crate::ConformanceResult as Result;
use serde_json::{Value, json};
/// Deliberately no Debug: failing assertions must not print signed material.
pub struct StatementRefusalCase {
    pub name: &'static str,
    pub statement: String,
    pub refusal: &'static str,
    pub status: u16,
}

fn with_claim(organization: &str, now_seconds: i64, field: &str, value: Value) -> Result<String> {
    let mut claims = super::claims(organization, now_seconds);
    super::set_claim(&mut claims, field, value)?;
    super::sign(&super::stamp_header(), &claims, 7)
}

pub fn statement_with_header(
    organization: &str,
    now_seconds: i64,
    header: Value,
    seed: u8,
) -> Result<String> {
    super::sign(&header, &super::claims(organization, now_seconds), seed)
}

pub fn statement_with_times(
    organization: &str,
    now_seconds: i64,
    issued: i64,
    expires: i64,
) -> Result<String> {
    let mut claims = super::claims(organization, now_seconds);
    super::set_claim(&mut claims, "iat", json!(issued))?;
    super::set_claim(&mut claims, "exp", json!(expires))?;
    super::sign(&super::stamp_header(), &claims, 7)
}

/// The historical seven profile cases and six further refusal cases, each
/// generated for a fresh addressed organization and verified with real keys.
pub fn statement_refusal_case(
    index: usize,
    organization: &str,
    now_seconds: i64,
) -> Result<StatementRefusalCase> {
    if index >= 7 {
        return further_refusal_case(index, organization, now_seconds);
    }
    let (name, statement, refusal, status) = match index {
        0 => (
            "edited signature",
            super::tamper_signature(&super::stamp(organization, now_seconds)?)?,
            "BadSignature",
            422,
        ),
        1 => (
            "wrong purpose",
            with_claim(organization, now_seconds, "purpose", json!("fleet-person"))?,
            "WrongPurpose",
            422,
        ),
        2 => (
            "wrong audience",
            with_claim(
                organization,
                now_seconds,
                "aud",
                json!("https://other.invalid"),
            )?,
            "WrongAudience",
            422,
        ),
        3 => (
            "wrong issuer",
            with_claim(
                organization,
                now_seconds,
                "iss",
                json!("https://other.invalid"),
            )?,
            "WrongIssuer",
            422,
        ),
        4 => (
            "longer than 120 days",
            with_claim(
                organization,
                now_seconds,
                "exp",
                json!(now_seconds - 60 + super::LIFETIME_SECONDS + 1),
            )?,
            "LifetimeExceedsPolicy",
            422,
        ),
        5 => (
            "wrong type",
            statement_with_header(
                organization,
                now_seconds,
                json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
                7,
            )?,
            "WrongTokenType",
            422,
        ),
        6 => (
            "unpublished key",
            statement_with_header(
                organization,
                now_seconds,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-z"}),
                9,
            )?,
            "BadSignature",
            422,
        ),
        _ => return Err("unknown profile refusal fixture case".into()),
    };
    Ok(StatementRefusalCase {
        name,
        statement,
        refusal,
        status,
    })
}

fn further_refusal_case(
    index: usize,
    organization: &str,
    now_seconds: i64,
) -> Result<StatementRefusalCase> {
    let (name, statement, refusal, status) = match index {
        7 => (
            "empty key ID",
            statement_with_header(
                organization,
                now_seconds,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":""}),
                7,
            )?,
            "Malformed",
            422,
        ),
        8 => (
            "unsupported algorithm",
            statement_with_header(
                organization,
                now_seconds,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"RS256","kid":"stamp-a"}),
                7,
            )?,
            "UnsupportedAlgorithm",
            422,
        ),
        9 => (
            "unsupported version",
            with_claim(organization, now_seconds, "version", json!(2))?,
            "UnsupportedVersion",
            422,
        ),
        10 => (
            "another organization's statement",
            super::stamp("019f0000-0000-7000-8000-00000000b2b1", now_seconds)?,
            "OrganizationMismatch",
            403,
        ),
        11 => (
            "expired",
            statement_with_times(
                organization,
                now_seconds,
                now_seconds - 200,
                now_seconds - 100,
            )?,
            "Expired",
            422,
        ),
        12 => (
            "not yet valid",
            statement_with_times(
                organization,
                now_seconds,
                now_seconds + 1_000,
                now_seconds + 2_000,
            )?,
            "NotYetValid",
            422,
        ),
        _ => return Err("unknown refusal fixture case".into()),
    };
    Ok(StatementRefusalCase {
        name,
        statement,
        refusal,
        status,
    })
}
