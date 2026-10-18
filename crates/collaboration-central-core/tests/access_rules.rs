#![allow(clippy::expect_used)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::*;
fn policy() -> AccessPolicy {
    AccessPolicy {
        team_domain: "test.cloudflareaccess.com".into(),
        session_audience: "session-aud".into(),
        admin_audience: "admin-aud".into(),
        owner_identity: "owner@example.test".into(),
    }
}
fn now() -> RegistryTime {
    RegistryTime(chrono::DateTime::from_timestamp(1000, 0).expect("time"))
}
fn claims() -> serde_json::Value {
    serde_json::json!({"type":"app","sub":"","common_name":"client-1","iss":"https://test.cloudflareaccess.com","aud":["session-aud"],"exp":1001,"nbf":1000})
}
fn token(claims: &serde_json::Value, alg: &str) -> String {
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(
            serde_json::to_vec(&serde_json::json!({"alg":alg,"kid":"key-1"})).expect("header")
        ),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).expect("claims")),
        URL_SAFE_NO_PAD.encode([1, 2, 3])
    )
}
#[test]
fn supplied_clock_exact_expiry_and_not_before_boundaries_are_enforced() {
    assert!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims(), "RS256")),
            &policy(),
            "/v1/session",
            now()
        )
        .is_ok()
    );
    for (field, value, reason) in [
        ("exp", 1000, AccessRefusal::Expired),
        ("nbf", 1001, AccessRefusal::NotYetValid),
    ] {
        let mut claims = claims();
        set_claim(&mut claims, field, serde_json::json!(value));
        assert_eq!(
            ParsedAccessAssertion::parse(
                Some(&token(&claims, "RS256")),
                &policy(),
                "/v1/session",
                now()
            )
            .err(),
            Some(reason)
        );
    }
}
/// The service-token assertion Cloudflare Access really issues, as observed
/// on M3.10 staging (claim names and types only): `aud` is a single string,
/// not the documented array, and there is no `nbf`. JWT allows either form of
/// `aud`; identity tokens use the array.
#[test]
fn observed_service_token_payload_parses_as_machine() {
    let claims = serde_json::json!({"type":"app","aud":"session-aud","exp":1001,"iss":"https://test.cloudflareaccess.com","common_name":"client-1","iat":999,"sub":"","h_INTERNAL_DO_NOT_USE":"x"});
    let parsed = ParsedAccessAssertion::parse(
        Some(&token(&claims, "RS256")),
        &policy(),
        "/v1/session",
        now(),
    )
    .expect("observed service-token payload");
    assert_eq!(
        parsed.verified(SignatureValidity::Valid),
        Ok(AccessCaller::Machine {
            client_id: AccessClientId::new("client-1").expect("client ID")
        })
    );
}
#[test]
fn single_string_audience_for_another_app_is_refused() {
    let claims = serde_json::json!({"type":"app","aud":"admin-aud","exp":1001,"iss":"https://test.cloudflareaccess.com","common_name":"client-1","iat":999,"sub":""});
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims, "RS256")),
            &policy(),
            "/v1/session",
            now()
        )
        .err(),
        Some(AccessRefusal::WrongAudience)
    );
}
#[test]
fn missing_malformed_algorithm_issuer_audience_and_path_fail_closed() {
    assert_eq!(
        ParsedAccessAssertion::parse(None, &policy(), "/v1/session", now()).err(),
        Some(AccessRefusal::MissingAssertion)
    );
    assert_eq!(
        ParsedAccessAssertion::parse(Some("bad.jwt"), &policy(), "/v1/session", now()).err(),
        Some(AccessRefusal::MalformedAssertion)
    );
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims(), "none")),
            &policy(),
            "/v1/session",
            now()
        )
        .err(),
        Some(AccessRefusal::UnsupportedAlgorithm)
    );
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims(), "RS256")),
            &policy(),
            "/admin/v1/enroll",
            now()
        )
        .err(),
        Some(AccessRefusal::WrongAudience)
    );
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims(), "RS256")),
            &policy(),
            "/anything",
            now()
        )
        .err(),
        Some(AccessRefusal::RouteRefused)
    );
    let mut claims = claims();
    set_claim(
        &mut claims,
        "iss",
        serde_json::json!("https://other.cloudflareaccess.com"),
    );
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims, "RS256")),
            &policy(),
            "/v1/session",
            now()
        )
        .err(),
        Some(AccessRefusal::WrongIssuer)
    );
}
#[test]
fn caller_is_not_released_until_the_signature_port_returns_valid() {
    let parsed = ParsedAccessAssertion::parse(
        Some(&token(&claims(), "RS256")),
        &policy(),
        "/v1/session",
        now(),
    )
    .expect("parsed");
    assert_eq!(parsed.kid(), "key-1");
    assert_eq!(parsed.signature(), &[1, 2, 3]);
    assert_eq!(
        parsed.verified(SignatureValidity::Invalid),
        Err(AccessRefusal::BadSignature)
    );
    let parsed = ParsedAccessAssertion::parse(
        Some(&token(&claims(), "RS256")),
        &policy(),
        "/v1/session",
        now(),
    )
    .expect("parsed");
    let caller = parsed.verified(SignatureValidity::Valid).expect("verified");
    assert_eq!(
        caller,
        AccessCaller::Machine {
            client_id: AccessClientId::new("client-1").expect("client")
        }
    );
    assert_eq!(
        authorize_access_route(&policy(), "/admin/v1/enroll", &caller),
        Err(AccessRefusal::WrongCallerClass)
    );
}
#[test]
fn owner_is_an_identity_assertion_and_must_match_the_configured_identity() {
    let mut claims = claims();
    claims
        .as_object_mut()
        .expect("claims")
        .remove("common_name");
    set_claim(
        &mut claims,
        "email",
        serde_json::json!("owner@example.test"),
    );
    set_claim(&mut claims, "sub", serde_json::json!("google-subject"));
    set_claim(&mut claims, "aud", serde_json::json!(["admin-aud"]));
    let caller = ParsedAccessAssertion::parse(
        Some(&token(&claims, "RS256")),
        &policy(),
        "/admin/v1/enroll",
        now(),
    )
    .expect("parsed")
    .verified(SignatureValidity::Valid)
    .expect("verified");
    assert_eq!(
        authorize_access_route(&policy(), "/admin/v1/enroll", &caller),
        Ok(())
    );
    assert_eq!(
        authorize_access_route(&policy(), "/v1/session", &caller),
        Err(AccessRefusal::WrongCallerClass)
    );
    assert_eq!(
        authorize_access_route(
            &policy(),
            "/admin/v1/enroll",
            &AccessCaller::Owner {
                email: "other@example.test".into()
            }
        ),
        Err(AccessRefusal::WrongOwner)
    );
    set_claim(&mut claims, "common_name", serde_json::json!("client-1"));
    assert_eq!(
        ParsedAccessAssertion::parse(
            Some(&token(&claims, "RS256")),
            &policy(),
            "/admin/v1/enroll",
            now()
        )
        .err(),
        Some(AccessRefusal::InvalidCaller)
    );
}
#[test]
fn jwks_selects_only_one_compatible_key_by_kid() {
    let key = AccessJwk {
        kid: "key-1".into(),
        kty: "RSA".into(),
        n: "modulus".into(),
        e: "AQAB".into(),
        alg: Some("RS256".into()),
        key_use: Some("sig".into()),
    };
    let keys = AccessJwks {
        keys: vec![key.clone()],
    };
    assert!(keys.key("key-1").is_ok());
    assert_eq!(keys.key("unknown").err(), Some(AccessRefusal::UnknownKey));
    assert_eq!(
        AccessJwks {
            keys: vec![key.clone(), key.clone()]
        }
        .key("key-1")
        .err(),
        Some(AccessRefusal::InvalidKey)
    );
    let mut wrong = key;
    wrong.alg = Some("HS256".into());
    assert_eq!(
        AccessJwks { keys: vec![wrong] }.key("key-1").err(),
        Some(AccessRefusal::InvalidKey)
    );
}

fn set_claim(claims: &mut serde_json::Value, name: &str, value: serde_json::Value) {
    claims
        .as_object_mut()
        .expect("fixture claims object")
        .insert(name.into(), value);
}
