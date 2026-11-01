//! B3's pinned structural examples through the Rust consumer, without real tokens.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail cross-language contract regressions while fixture errors propagate."
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignatureValidity,
    organization_authority::{
        ParsedPersonToken, PersonTokenAudience, PersonTokenPolicy, PersonTokenRefusal,
        VerifiedPersonToken,
    },
};
use fleet_identity_protocol::UnixMillis;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize)]
struct ProducerExamples {
    contract: String,
    examples: BTreeMap<String, Value>,
}

fn examples(source: &str) -> Result<ProducerExamples, serde_json::Error> {
    serde_json::from_str(source)
}

fn k6_examples() -> Result<ProducerExamples, serde_json::Error> {
    examples(include_str!("fixtures/person-identity/K6.fixtures.json"))
}

fn k6_reader(claims: &Value) -> Result<VerifiedPersonToken, PersonTokenRefusal> {
    let header = serde_json::to_vec(&json!({"typ":"at+jwt","alg":"EdDSA","kid":"fixture-key"}))
        .map_err(|_| PersonTokenRefusal::Malformed)?;
    let body = serde_json::to_vec(claims).map_err(|_| PersonTokenRefusal::Malformed)?;
    let wrapped = format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(header),
        URL_SAFE_NO_PAD.encode(body),
        URL_SAFE_NO_PAD.encode([1_u8, 2, 3]),
    );
    let policy = PersonTokenPolicy::new(
        "https://sign-in.example.invalid".into(),
        "https://central.example.invalid/admin".into(),
        "https://central.example.invalid/operator".into(),
        "person-admin-client".into(),
    )?;
    ParsedPersonToken::parse(wrapped)?.verified(
        SignatureValidity::Valid,
        &policy,
        PersonTokenAudience::Admin,
        UnixMillis(2_208_988_800_000),
    )
}

#[test]
fn organization_authority_producer_examples_validate_against_their_owned_schemas() -> TestResult {
    for (name, schema, fixtures) in [
        (
            "K5SessionConfirmation",
            include_str!("fixtures/person-identity/K5.schema.json"),
            include_str!("fixtures/person-identity/K5.fixtures.json"),
        ),
        (
            "K6OrganizationPersonTokenClaims",
            include_str!("fixtures/person-identity/K6.schema.json"),
            include_str!("fixtures/person-identity/K6.fixtures.json"),
        ),
    ] {
        let fixtures = examples(fixtures)?;
        assert_eq!(fixtures.contract, name);
        assert!(
            !fixtures.examples.is_empty(),
            "producer examples were not discovered"
        );
        let schema: Value = serde_json::from_str(schema)?;
        let validator = jsonschema::options().build(&schema)?;
        for (example_name, value) in fixtures.examples {
            assert!(
                validator.is_valid(&value),
                "{name}/{example_name} violates its producer-owned schema"
            );
        }
    }
    Ok(())
}

#[test]
fn organization_authority_k6_reads_each_staged_producer_example() -> TestResult {
    let fixtures = k6_examples()?;
    assert_eq!(fixtures.contract, "K6OrganizationPersonTokenClaims");
    assert!(!fixtures.examples.is_empty());
    for (name, claims) in fixtures.examples {
        let token = k6_reader(&claims)?;
        assert_eq!(
            token.person_id().as_str(),
            "11111111-1111-7111-8111-111111111111",
            "{name}"
        );
        assert_eq!(
            token.session_ref().as_str(),
            "22222222-2222-7222-8222-222222222222",
            "{name}"
        );
        assert_eq!(
            token.organization_id().as_str(),
            "33333333-3333-7333-8333-333333333333",
            "{name}"
        );
        assert_eq!(token.expires_at().0, 2_208_989_400, "{name}");
    }
    Ok(())
}

#[test]
fn organization_authority_k6_refuses_invalid_sessions_times_and_methods() -> TestResult {
    let base = k6_examples()?
        .examples
        .remove("providerSession")
        .ok_or("named producer example missing")?;
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/person-identity/K6.schema.json"))?;
    let validator = jsonschema::options().build(&schema)?;
    let mut accepted = Vec::new();
    for (name, field, value) in [
        ("opaque-session", "sid", json!("issuer-owned-session")),
        (
            "v4-session",
            "sid",
            json!("22222222-2222-4222-8222-222222222222"),
        ),
        ("negative-issue", "iat", json!(-1)),
        ("unsafe-expiry", "exp", json!(9_007_199_254_740_992_i64)),
        ("negative-factor", "second_factor_at", json!(-1)),
        (
            "unsafe-factor",
            "second_factor_at",
            json!(9_007_199_254_740_992_i64),
        ),
        ("unknown-method", "amr", json!(["unknown"])),
    ] {
        let mut claims = base.clone();
        claims
            .as_object_mut()
            .ok_or("producer claims are not an object")?
            .extend([(field.into(), value)]);
        assert!(
            !validator.is_valid(&claims),
            "{name} did not reach a forbidden producer shape"
        );
        if k6_reader(&claims).is_ok() {
            accepted.push(name);
        }
    }
    assert!(
        accepted.is_empty(),
        "Rust accepted invalid producer shapes: {accepted:?}"
    );
    Ok(())
}
