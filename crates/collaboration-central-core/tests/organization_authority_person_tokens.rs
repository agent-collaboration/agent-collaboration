//! Person-token policy at the core boundary; cryptographic validity is injected.
//!
//! Oracles: item-two R13 and the inherited person-proof verification contract.
//! These cases cannot establish F3a's real signature/key-fetch integration.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail policy regressions while fixture errors propagate."
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignatureValidity,
    organization_authority::{
        ParsedPersonToken, PersonTokenAudience, PersonTokenPolicy, PersonTokenRefusal,
    },
};
use fleet_identity_protocol::UnixMillis;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

const ISSUER: &str = "https://signin.example.invalid";
const ADMIN: &str = "https://central.example.invalid/v1/admin";
const OPERATOR: &str = "https://central.example.invalid/v1/operator";
const PERSON: &str = "019a3dce-2000-7000-8000-000000000001";
const ORGANIZATION: &str = "019a3dce-2000-7000-8000-000000000002";
const NOW: UnixMillis = UnixMillis(1_800_000_000_000);

fn claims() -> Value {
    json!({
        "iss": ISSUER, "sub": PERSON, "aud": ADMIN,
        "exp": 1_800_000_300, "iat": 1_800_000_000,
        "scope": "central:admin", "client_id": "sign-in-app",
        "azp": "sign-in-app", "sid": "issuer-owned-session",
        "jti": "issuer-owned-token", "organization_id": ORGANIZATION,
        "second_factor_at": 1_799_999_950, "amr": ["otp"],
    })
}

fn policy() -> Result<PersonTokenPolicy, PersonTokenRefusal> {
    PersonTokenPolicy::new(
        ISSUER.into(),
        ADMIN.into(),
        OPERATOR.into(),
        "sign-in-app".into(),
    )
}

fn token(header: &Value, body: &Value) -> Result<String, serde_json::Error> {
    Ok(format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(body)?),
        URL_SAFE_NO_PAD.encode([1_u8, 2, 3]),
    ))
}

fn person_token(
    body: &Value,
) -> Result<ParsedPersonToken, Box<dyn std::error::Error + Send + Sync>> {
    Ok(ParsedPersonToken::parse(token(
        &json!({"typ":"at+jwt", "alg":"EdDSA", "kid":"issuer-key"}),
        body,
    )?)?)
}

#[test]
fn organization_authority_person_policy_accepts_a_person_and_preserves_signed_bytes() -> TestResult
{
    let body = claims();
    let header = json!({"typ":"at+jwt", "alg":"EdDSA", "kid":"issuer-key"});
    let encoded = token(&header, &body)?;
    let parsed = ParsedPersonToken::parse(encoded.clone())?;
    let input = parsed.signature_input();
    assert_eq!(input.key_id(), "issuer-key");
    assert_eq!(input.signature(), [1, 2, 3]);
    assert_eq!(
        input.signed_bytes(),
        encoded
            .rsplit_once('.')
            .ok_or("JWT delimiter")?
            .0
            .as_bytes()
    );
    let verified = parsed.verified(
        SignatureValidity::Valid,
        &policy()?,
        PersonTokenAudience::Admin,
        NOW,
    )?;
    assert_eq!(verified.person_id().as_str(), PERSON);
    assert_eq!(verified.organization_id().as_str(), ORGANIZATION);
    assert_eq!(verified.session_ref().as_str(), "issuer-owned-session");
    assert_eq!(
        verified.second_factor_at().map(|at| at.0),
        Some(1_799_999_950)
    );
    Ok(())
}

#[test]
fn organization_authority_person_policy_never_promotes_an_invalid_signature() -> TestResult {
    assert!(matches!(
        person_token(&claims())?.verified(
            SignatureValidity::Invalid,
            &policy()?,
            PersonTokenAudience::Admin,
            NOW
        ),
        Err(PersonTokenRefusal::BadSignature)
    ));
    Ok(())
}

#[test]
fn organization_authority_person_policy_refuses_each_wrong_proof_kind() -> TestResult {
    for (field, value, expected) in [
        (
            "iss",
            json!("https://other.example.invalid"),
            PersonTokenRefusal::WrongIssuer,
        ),
        ("aud", json!(OPERATOR), PersonTokenRefusal::WrongAudience),
        ("aud", json!([ADMIN]), PersonTokenRefusal::WrongAudience),
        ("sid", Value::Null, PersonTokenRefusal::NotAPerson),
        (
            "organization_id",
            Value::Null,
            PersonTokenRefusal::NoOrganization,
        ),
    ] {
        let mut body = claims();
        body[field] = value;
        assert!(
            matches!(
                person_token(&body)?.verified(SignatureValidity::Valid, &policy()?, PersonTokenAudience::Admin, NOW),
                Err(actual) if actual == expected
            ),
            "wrong proof field {field} was accepted"
        );
    }
    Ok(())
}

#[test]
fn organization_authority_operator_policy_requires_its_pinned_client() -> TestResult {
    let mut body = claims();
    body["aud"] = json!(OPERATOR);
    person_token(&body)?.verified(
        SignatureValidity::Valid,
        &policy()?,
        PersonTokenAudience::Operator,
        NOW,
    )?;
    for field in ["client_id", "azp"] {
        let mut changed = body.clone();
        changed[field] = json!("another-client");
        assert!(matches!(
            person_token(&changed)?.verified(
                SignatureValidity::Valid,
                &policy()?,
                PersonTokenAudience::Operator,
                NOW
            ),
            Err(PersonTokenRefusal::WrongClient)
        ));
    }
    Ok(())
}

#[test]
fn organization_authority_token_time_checks_use_the_declared_sixty_second_skew() -> TestResult {
    let mut body = claims();
    body["iat"] = json!(1_799_999_700);
    body["exp"] = json!(1_799_999_941);
    person_token(&body)?.verified(
        SignatureValidity::Valid,
        &policy()?,
        PersonTokenAudience::Admin,
        NOW,
    )?;
    body["exp"] = json!(1_799_999_940);
    assert!(matches!(
        person_token(&body)?.verified(
            SignatureValidity::Valid,
            &policy()?,
            PersonTokenAudience::Admin,
            NOW
        ),
        Err(PersonTokenRefusal::Expired)
    ));
    let mut body = claims();
    body["iat"] = json!(1_800_000_060);
    person_token(&body)?.verified(
        SignatureValidity::Valid,
        &policy()?,
        PersonTokenAudience::Admin,
        NOW,
    )?;
    body["iat"] = json!(1_800_000_061);
    assert!(matches!(
        person_token(&body)?.verified(
            SignatureValidity::Valid,
            &policy()?,
            PersonTokenAudience::Admin,
            NOW
        ),
        Err(PersonTokenRefusal::NotYetValid)
    ));
    Ok(())
}

#[test]
fn organization_authority_token_parser_refuses_wrong_headers_and_extra_segments() -> TestResult {
    for (header, expected) in [
        (
            json!({"typ":"JWT","alg":"EdDSA","kid":"key"}),
            PersonTokenRefusal::WrongTokenType,
        ),
        (
            json!({"typ":"at+jwt","alg":"none","kid":"key"}),
            PersonTokenRefusal::UnsupportedAlgorithm,
        ),
        (
            json!({"typ":"at+jwt","alg":"EdDSA","kid":""}),
            PersonTokenRefusal::Malformed,
        ),
    ] {
        assert!(
            matches!(ParsedPersonToken::parse(token(&header, &claims())?), Err(actual) if actual == expected)
        );
    }
    let encoded = token(
        &json!({"typ":"at+jwt","alg":"EdDSA","kid":"key"}),
        &claims(),
    )?;
    assert!(matches!(
        ParsedPersonToken::parse(format!("{encoded}.extra")),
        Err(PersonTokenRefusal::Malformed)
    ));
    Ok(())
}

#[test]
fn organization_authority_person_claims_allow_registered_extras_and_no_factor_for_a_read()
-> TestResult {
    let mut body = claims();
    body.as_object_mut()
        .ok_or("claims object")?
        .remove("second_factor_at");
    body["registered_extension"] = json!({"anything":"owned by the issuer"});
    let verified = person_token(&body)?.verified(
        SignatureValidity::Valid,
        &policy()?,
        PersonTokenAudience::Admin,
        NOW,
    )?;
    assert_eq!(verified.second_factor_at(), None);
    Ok(())
}
