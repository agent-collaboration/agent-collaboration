//! Organization statements a Fleet must refuse, each paired with the closed
//! refusal kind central's verifier yields for it (organization_authority/
//! fleet_statement.rs `verify`; an unknown key ID verifies as invalid).
use crate::organization_stamps::{self as stamps, LIFETIME_SECONDS, NOW_SECONDS};
use collaboration_central_conformance::ConformanceResult;
use serde_json::{Value, json};

/// One refused statement: a label, the statement, and the refusal's `Debug`
/// name as central logs it.
pub(crate) struct StatementRefusal {
    pub(crate) case: &'static str,
    pub(crate) statement: String,
    pub(crate) refusal: &'static str,
}

fn with_claim(organization: &str, field: &str, value: Value) -> ConformanceResult<String> {
    let mut claims = stamps::claims(organization);
    stamps::set_claim(&mut claims, field, value)?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

fn with_header(organization: &str, header: Value, seed: u8) -> ConformanceResult<String> {
    stamps::sign(&header, &stamps::claims(organization), seed)
}

fn with_times(organization: &str, issued: i64, expires: i64) -> ConformanceResult<String> {
    let mut claims = stamps::claims(organization);
    stamps::set_claim(&mut claims, "iat", json!(issued))?;
    stamps::set_claim(&mut claims, "exp", json!(expires))?;
    stamps::sign(&stamps::stamp_header(), &claims, 7)
}

/// The stamp-profile refusals, each correctly signed but for one field. A
/// Fleet answers every one of these with 422.
pub(crate) fn profile_refusals(organization: &str) -> ConformanceResult<Vec<StatementRefusal>> {
    Ok(vec![
        StatementRefusal {
            case: "edited signature",
            statement: stamps::tamper_signature(&stamps::stamp(organization)?)?,
            refusal: "BadSignature",
        },
        StatementRefusal {
            case: "wrong purpose",
            statement: with_claim(organization, "purpose", json!("fleet-person"))?,
            refusal: "WrongPurpose",
        },
        StatementRefusal {
            case: "wrong audience",
            statement: with_claim(organization, "aud", json!("https://other.invalid"))?,
            refusal: "WrongAudience",
        },
        StatementRefusal {
            case: "wrong issuer",
            statement: with_claim(organization, "iss", json!("https://other.invalid"))?,
            refusal: "WrongIssuer",
        },
        StatementRefusal {
            case: "longer than 120 days",
            statement: with_claim(
                organization,
                "exp",
                json!(NOW_SECONDS - 60 + LIFETIME_SECONDS + 1),
            )?,
            refusal: "LifetimeExceedsPolicy",
        },
        StatementRefusal {
            case: "wrong type",
            statement: with_header(
                organization,
                json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
                7,
            )?,
            refusal: "WrongTokenType",
        },
        StatementRefusal {
            case: "unpublished key",
            statement: with_header(
                organization,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-z"}),
                9,
            )?,
            refusal: "BadSignature",
        },
    ])
}

/// The remaining refusal kinds a statement can draw at verification. All but
/// another organization's statement (403) answer 422.
pub(crate) fn further_refusals(
    organization: &str,
    other_organization: &str,
) -> ConformanceResult<Vec<StatementRefusal>> {
    Ok(vec![
        StatementRefusal {
            case: "empty key ID",
            statement: with_header(
                organization,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":""}),
                7,
            )?,
            refusal: "Malformed",
        },
        StatementRefusal {
            case: "unsupported algorithm",
            statement: with_header(
                organization,
                json!({"typ":"fleet-organization-stamp+jwt","alg":"RS256","kid":"stamp-a"}),
                7,
            )?,
            refusal: "UnsupportedAlgorithm",
        },
        StatementRefusal {
            case: "unsupported version",
            statement: with_claim(organization, "version", json!(2))?,
            refusal: "UnsupportedVersion",
        },
        StatementRefusal {
            case: "another organization's statement",
            statement: stamps::stamp(other_organization)?,
            refusal: "OrganizationMismatch",
        },
        StatementRefusal {
            case: "expired",
            statement: with_times(organization, NOW_SECONDS - 200, NOW_SECONDS - 100)?,
            refusal: "Expired",
        },
        StatementRefusal {
            case: "not yet valid",
            statement: with_times(organization, NOW_SECONDS + 1_000, NOW_SECONDS + 2_000)?,
            refusal: "NotYetValid",
        },
    ])
}
