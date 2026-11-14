use super::FleetOrganizationStatementRefusal as Refusal;
use crate::organization_authority::fleet_statement_test_support::{
    FOREIGN_ORGANIZATION, ORGANIZATION, claims, header, organization, signed_statement, verifier,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn fleet_statement_accepts_a_real_signature_with_the_exact_sign_in_profile() {
    let (verifier, _) = verifier();
    let statement = signed_statement(&header(), &claims());
    let verified = verifier.verify(&statement).await.unwrap();
    assert_eq!(verified.organization_id(), &organization(ORGANIZATION));
    assert_eq!(verified.key_id(), "stamp-fixture");
    assert_eq!(verified.expires_at().0, 1_800_000_600);
    assert!(verified.replaces().is_none());
}

#[tokio::test]
async fn fleet_statement_refuses_a_forged_signature() {
    let (verifier, _) = verifier();
    let valid = signed_statement(&header(), &claims());
    let (signed, _) = valid.rsplit_once('.').unwrap();
    let forged = format!("{signed}.{}", URL_SAFE_NO_PAD.encode([0; 64]));
    assert_eq!(
        verifier.verify(&forged).await.unwrap_err(),
        Refusal::BadSignature
    );
}

#[tokio::test]
async fn fleet_statement_refuses_an_edited_signed_predecessor() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["replaces"] = json!("ab".repeat(32));
    let original = signed_statement(&header(), &input);
    let (_, signature) = original.rsplit_once('.').unwrap();
    input["replaces"] = json!("cd".repeat(32));
    let edited = format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header()).unwrap()),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&input).unwrap()),
        signature
    );

    let result = verifier.verify(&edited).await;

    assert_eq!(result.unwrap_err(), Refusal::BadSignature);
}

#[tokio::test]
async fn fleet_statement_refuses_a_person_token_header() {
    let (verifier, _) = verifier();
    let mut input = header();
    input["typ"] = json!("at+jwt");
    let statement = signed_statement(&input, &claims());
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::WrongTokenType
    );
}

#[tokio::test]
async fn fleet_statement_refuses_another_signed_purpose() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["purpose"] = json!("person-token");
    let statement = signed_statement(&header(), &input);
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::WrongPurpose
    );
}

#[tokio::test]
async fn fleet_statement_refuses_another_audience() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["aud"] = json!("https://another.example.invalid/identity");
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::WrongAudience
    );
}

#[tokio::test]
async fn fleet_statement_expires_at_the_exact_exp_boundary() {
    let (verifier, clock) = verifier();
    let statement = signed_statement(&header(), &claims());
    clock.0.store(1_800_000_599_999, Ordering::SeqCst);
    assert!(verifier.verify(&statement).await.is_ok());
    clock.0.store(1_800_000_600_000, Ordering::SeqCst);
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::Expired
    );
}

#[tokio::test]
async fn fleet_statement_refuses_another_organization_even_with_its_real_signature() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["organization_id"] = json!(FOREIGN_ORGANIZATION);
    input["sub"] = json!(FOREIGN_ORGANIZATION);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::OrganizationMismatch
    );
}

#[tokio::test]
async fn fleet_statement_requires_subject_and_organization_to_agree() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["sub"] = json!(FOREIGN_ORGANIZATION);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::OrganizationMismatch
    );
}

#[tokio::test]
async fn fleet_statement_refuses_a_different_issuer() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["iss"] = json!("https://another.example.invalid");
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::WrongIssuer
    );
}

#[tokio::test]
async fn fleet_statement_refuses_an_unsupported_version() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["version"] = json!(2);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::UnsupportedVersion
    );
}

#[tokio::test]
async fn fleet_statement_refuses_a_future_issuance_time() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["iat"] = json!(1_800_000_001);
    assert_eq!(
        verifier
            .verify(&signed_statement(&header(), &input))
            .await
            .unwrap_err(),
        Refusal::NotYetValid
    );
}

#[tokio::test]
async fn fleet_statement_refuses_another_signature_algorithm() {
    let (verifier, _) = verifier();
    let mut input = header();
    input["alg"] = json!("RS256");
    assert_eq!(
        verifier
            .verify(&signed_statement(&input, &claims()))
            .await
            .unwrap_err(),
        Refusal::UnsupportedAlgorithm
    );
}

#[tokio::test]
async fn fleet_statement_keeps_a_valid_signed_predecessor_without_applying_policy() {
    let (verifier, _) = verifier();
    let mut input = claims();
    input["replaces"] = json!("ab".repeat(32));
    let verified = verifier
        .verify(&signed_statement(&header(), &input))
        .await
        .unwrap();
    assert_eq!(verified.replaces().unwrap().as_str(), "ab".repeat(32));
}

#[tokio::test]
async fn fleet_statement_refuses_null_malformed_or_unsigned_profile_fields() {
    let (verifier, _) = verifier();
    for (field, value) in [
        ("replaces", Value::Null),
        ("replaces", json!("AB".repeat(32))),
        ("sid", json!("person-session")),
        ("aud", json!(["https://central.example.invalid/identity"])),
        ("exp", json!(9_007_199_254_740_992_i64)),
        ("iat", json!(-1)),
    ] {
        let mut input = claims();
        input[field] = value;
        assert_eq!(
            verifier
                .verify(&signed_statement(&header(), &input))
                .await
                .unwrap_err(),
            Refusal::Malformed
        );
    }
    let mut input = header();
    input["crit"] = json!(["future"]);
    assert_eq!(
        verifier
            .verify(&signed_statement(&input, &claims()))
            .await
            .unwrap_err(),
        Refusal::Malformed
    );
}

#[tokio::test]
async fn stored_facts_keep_expired_bounds_without_changing_normal_verification() {
    // Arrange: the same real signature is read at and after its signed expiry.
    let (verifier, clock) = verifier();
    let statement = signed_statement(&header(), &claims());
    for now in [1_800_000_600_000, 1_800_000_601_000] {
        clock.0.store(now, Ordering::SeqCst);

        // Act: facts verification shares the real signature/profile verifier.
        let facts = verifier
            .verify_stored_statement_facts(&statement)
            .await
            .unwrap();

        // Assert: the factual result grants no current validity to any caller.
        assert_eq!(facts.organization_id(), &organization(ORGANIZATION));
        assert_eq!(facts.key_id(), "stamp-fixture");
        assert_eq!(facts.expires_at().0, 1_800_000_600);
        assert_eq!(
            facts.require_valid_at(runtime_primitives::SyncTime::from_unix_millis(now)),
            Err(Refusal::Expired)
        );
        assert_eq!(
            verifier.verify(&statement).await.unwrap_err(),
            Refusal::Expired
        );
    }
}

#[tokio::test]
async fn stored_facts_keep_future_issuance_without_changing_normal_verification() {
    // Arrange: a valid signature whose issuance is after the injected clock.
    let (verifier, _) = verifier();
    let mut input = claims();
    input["iat"] = json!(1_800_000_001);
    let statement = signed_statement(&header(), &input);

    // Act / Assert: rollback does not hide facts or authorize a future stamp.
    let facts = verifier
        .verify_stored_statement_facts(&statement)
        .await
        .unwrap();
    assert_eq!(facts.issued_at().0, 1_800_000_001);
    assert_eq!(facts.expires_at().0, 1_800_000_600);
    assert_eq!(
        facts.require_valid_at(runtime_primitives::SyncTime::from_unix_millis(
            1_800_000_000_000
        )),
        Err(Refusal::NotYetValid)
    );
    assert_eq!(
        verifier.verify(&statement).await.unwrap_err(),
        Refusal::NotYetValid
    );
}

#[tokio::test]
async fn stored_facts_and_normal_verification_share_signature_and_profile_refusals() {
    let (verifier, _) = verifier();
    let valid = signed_statement(&header(), &claims());
    let (signed, _) = valid.rsplit_once('.').unwrap();
    let forged = format!("{signed}.{}", URL_SAFE_NO_PAD.encode([0; 64]));
    assert_eq!(
        verifier
            .verify_stored_statement_facts(&forged)
            .await
            .unwrap_err(),
        Refusal::BadSignature
    );
    assert_eq!(
        verifier.verify(&forged).await.unwrap_err(),
        Refusal::BadSignature
    );

    // Every expected refusal is independent of current-time admission.
    for (field, value, expected) in [
        ("purpose", json!("person-token"), Refusal::WrongPurpose),
        ("version", json!(2), Refusal::UnsupportedVersion),
        (
            "iss",
            json!("https://another.example.invalid"),
            Refusal::WrongIssuer,
        ),
        (
            "aud",
            json!("https://another.example.invalid/identity"),
            Refusal::WrongAudience,
        ),
        (
            "organization_id",
            json!(FOREIGN_ORGANIZATION),
            Refusal::OrganizationMismatch,
        ),
        (
            "sub",
            json!(FOREIGN_ORGANIZATION),
            Refusal::OrganizationMismatch,
        ),
        ("exp", json!(1_799_999_900), Refusal::Malformed),
        (
            "exp",
            json!(1_799_999_900 + super::MAXIMUM_STATEMENT_LIFETIME_SECONDS + 1),
            Refusal::LifetimeExceedsPolicy,
        ),
        ("iat", json!(-1), Refusal::Malformed),
    ] {
        let mut input = claims();
        input[field] = value;
        let statement = signed_statement(&header(), &input);
        assert_eq!(
            verifier
                .verify_stored_statement_facts(&statement)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(verifier.verify(&statement).await.unwrap_err(), expected);
    }
    for (field, value, expected) in [
        ("typ", json!("at+jwt"), Refusal::WrongTokenType),
        ("alg", json!("RS256"), Refusal::UnsupportedAlgorithm),
        ("kid", json!(""), Refusal::Malformed),
        ("crit", json!(["future"]), Refusal::Malformed),
    ] {
        let mut input = header();
        input[field] = value;
        let statement = signed_statement(&input, &claims());
        assert_eq!(
            verifier
                .verify_stored_statement_facts(&statement)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(verifier.verify(&statement).await.unwrap_err(), expected);
    }
}
