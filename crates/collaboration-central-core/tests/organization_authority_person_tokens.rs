//! Person-token policy at the core boundary; cryptographic validity is injected.
//!
//! Oracles: item-two R13 and the inherited person-proof verification contract.
//! These cases cannot establish F3a's real signature/key-fetch integration.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail policy regressions while fixture errors propagate."
)]

use collaboration_central_core::{
    SignatureValidity,
    organization_authority::{
        ParsedPersonToken, PersonConfirmationRefusal, PersonConfirmationWindow,
        PersonSessionConfirmation, PersonTokenAudience, PersonTokenRefusal,
    },
};
use fleet_identity_protocol::UnixMillis;
use serde_json::{Value, json};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[path = "support/person_token_fixture.rs"]
mod person_token_fixture;
use person_token_fixture::{
    ADMIN, NOW, OPERATOR, ORGANIZATION, PERSON, SIGN_IN_SESSION, claims, person_token, policy,
    token,
};

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
    assert_eq!(verified.session_ref().as_str(), SIGN_IN_SESSION);
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
fn organization_authority_person_policy_accepts_registered_extensions_and_absent_factor()
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

fn confirmation() -> Result<PersonSessionConfirmation, Box<dyn std::error::Error + Send + Sync>> {
    Ok(PersonSessionConfirmation::Active {
        person_id: PERSON.to_owned().try_into()?,
        session_ref: SIGN_IN_SESSION.to_owned().try_into()?,
        audience: ADMIN.into(),
        organization_id: ORGANIZATION.to_owned().try_into()?,
        expires_at: fleet_identity_protocol::UnixSeconds(1_800_000_300),
    })
}

#[test]
fn organization_authority_confirmation_measures_its_own_receipt_to_apply_window() -> TestResult {
    for (elapsed_ms, accepted) in [(4_999, true), (5_001, false), (-1, false)] {
        let verified = person_token(&claims())?.verified(
            SignatureValidity::Valid,
            &policy()?,
            PersonTokenAudience::Admin,
            NOW,
        )?;
        let confirmed = verified.confirmed(confirmation()?, NOW)?;
        let applied_at = UnixMillis(
            NOW.0
                .checked_add(elapsed_ms)
                .ok_or("fixture clock overflow")?,
        );
        assert_eq!(
            confirmed
                .require_fresh(applied_at, PersonConfirmationWindow::default())
                .is_ok(),
            accepted,
            "receipt-to-apply elapsed {elapsed_ms} ms",
        );
    }
    Ok(())
}

#[test]
fn organization_authority_confirmation_refuses_inactive_or_mismatched_subjects() -> TestResult {
    for changed_field in [
        "inactive",
        "person",
        "session",
        "audience",
        "organization",
        "expiry",
    ] {
        let answer = match changed_field {
            "inactive" => PersonSessionConfirmation::Inactive {},
            field => PersonSessionConfirmation::Active {
                person_id: (if field == "person" {
                    ORGANIZATION
                } else {
                    PERSON
                })
                .to_owned()
                .try_into()?,
                session_ref: (if field == "session" {
                    "019a3dce-2000-7000-8000-000000000004"
                } else {
                    SIGN_IN_SESSION
                })
                .to_owned()
                .try_into()?,
                audience: (if field == "audience" { OPERATOR } else { ADMIN }).into(),
                organization_id: (if field == "organization" {
                    PERSON
                } else {
                    ORGANIZATION
                })
                .to_owned()
                .try_into()?,
                expires_at: fleet_identity_protocol::UnixSeconds(if field == "expiry" {
                    1_800_000_301
                } else {
                    1_800_000_300
                }),
            },
        };
        let verified = person_token(&claims())?.verified(
            SignatureValidity::Valid,
            &policy()?,
            PersonTokenAudience::Admin,
            NOW,
        )?;
        assert!(
            matches!(
                verified.confirmed(answer, NOW),
                Err(PersonConfirmationRefusal::Inactive
                    | PersonConfirmationRefusal::MismatchedProof)
            ),
            "confirmation mismatch in {changed_field} was admitted"
        );
    }
    Ok(())
}

#[test]
fn organization_authority_confirmation_window_requires_a_positive_setting() -> TestResult {
    assert!(matches!(
        PersonConfirmationWindow::from_millis(0),
        Err(PersonConfirmationRefusal::InvalidWindow)
    ));
    PersonConfirmationWindow::from_millis(5_000)?;
    Ok(())
}
