//! B3's pinned structural examples through the Rust consumer, without real tokens.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail cross-language contract regressions while fixture errors propagate."
)]

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignatureValidity,
    organization_authority::{
        ParsedPersonToken, PersonConfirmationWindow, PersonSessionConfirmation,
        PersonTokenAudience, PersonTokenPolicy, PersonTokenRefusal, VerifiedPersonToken,
        decode_person_session_confirmation,
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

#[test]
fn organization_authority_k5_reads_both_staged_reply_states_and_active_extensions() -> TestResult {
    let fixtures = examples(include_str!("fixtures/person-identity/K5.fixtures.json"))?;
    assert_eq!(fixtures.contract, "K5SessionConfirmation");
    let inactive = fixtures
        .examples
        .get("inactive")
        .ok_or("inactive producer example")?;
    assert!(matches!(
        decode_person_session_confirmation(&serde_json::to_vec(inactive)?),
        Ok(PersonSessionConfirmation::Inactive {})
    ));
    let mut active = fixtures
        .examples
        .get("active")
        .ok_or("active producer example")?
        .clone();
    active
        .as_object_mut()
        .ok_or("active example object")?
        .extend([("registered_extra".into(), json!("allowed"))]);
    let decoded = decode_person_session_confirmation(&serde_json::to_vec(&active)?)?;
    let PersonSessionConfirmation::Active {
        person_id,
        session_ref,
        organization_id,
        audience,
        expires_at,
    } = decoded
    else {
        return Err("active producer example became inactive".into());
    };
    assert_eq!(person_id.as_str(), "11111111-1111-7111-8111-111111111111");
    assert_eq!(session_ref.as_str(), "22222222-2222-7222-8222-222222222222");
    assert_eq!(
        organization_id.as_str(),
        "33333333-3333-7333-8333-333333333333"
    );
    assert_eq!(audience, "https://central.example.invalid/admin");
    assert_eq!(expires_at.0, 2_208_989_400);
    Ok(())
}

#[test]
fn organization_authority_k5_refuses_inactive_disclosure_and_malformed_active_values() -> TestResult
{
    let schema: Value =
        serde_json::from_str(include_str!("fixtures/person-identity/K5.schema.json"))?;
    let validator = jsonschema::options().build(&schema)?;
    let fixtures = examples(include_str!("fixtures/person-identity/K5.fixtures.json"))?;
    let active = fixtures
        .examples
        .get("active")
        .ok_or("active producer example")?;
    let mut missing = active.clone();
    missing
        .as_object_mut()
        .ok_or("active example object")?
        .remove("checkedAt");
    let mut invalid = vec![
        json!({"active":false,"sub":"11111111-1111-7111-8111-111111111111"}),
        json!({"active":true}),
        missing,
    ];
    for (field, value) in [
        ("checkedAt", json!(-1)),
        ("checkedAt", json!(9_007_199_254_740_992_i64)),
        ("exp", json!(-1)),
        ("sid", json!("opaque-session")),
        ("aud", json!(["https://central.example.invalid/admin"])),
        ("active", json!(false)),
    ] {
        let mut value_with_bad_field = active.clone();
        value_with_bad_field
            .as_object_mut()
            .ok_or("active example object")?
            .extend([(field.into(), value)]);
        invalid.push(value_with_bad_field);
    }
    for invalid_reply in invalid {
        assert!(
            !validator.is_valid(&invalid_reply),
            "invalid reply fixture did not reach a forbidden producer shape"
        );
        assert!(
            decode_person_session_confirmation(&serde_json::to_vec(&invalid_reply)?).is_err(),
            "malformed confirmation was accepted: {invalid_reply}"
        );
    }
    Ok(())
}

#[test]
fn organization_authority_k5_checked_at_does_not_replace_central_receipt_time() -> TestResult {
    let fixtures = examples(include_str!("fixtures/person-identity/K5.fixtures.json"))?;
    let active = fixtures
        .examples
        .get("active")
        .ok_or("active producer example")?;
    let claims = k6_examples()?
        .examples
        .remove("providerSession")
        .ok_or("person token example")?;
    for checked_at in [0_i64, 9_007_199_254_740_991_i64] {
        let mut changed = active.clone();
        changed
            .as_object_mut()
            .ok_or("active example object")?
            .extend([("checkedAt".into(), json!(checked_at))]);
        let answer = decode_person_session_confirmation(&serde_json::to_vec(&changed)?)?;
        let proof = k6_reader(&claims)?.confirmed(answer, UnixMillis(2_208_988_800_000))?;
        assert!(
            proof
                .require_fresh(
                    UnixMillis(2_208_988_804_999),
                    PersonConfirmationWindow::default()
                )
                .is_ok()
        );
    }
    Ok(())
}
