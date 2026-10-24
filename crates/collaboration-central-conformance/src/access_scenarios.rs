use crate::{
    AccessCallerClass, AccessVerificationOutcome, ConformanceClient, ConformanceResult,
    access_fixture::*,
    scenario_assertions::{check, equal},
};
use collaboration_central_core::*;

/// The same signed assertion must stop being valid at its own expiry.
pub async fn run_access_expiry_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut short = fixture_claims(
        AccessCaller::Machine {
            client_id: access_client_id("expiry-row-machine")?,
        },
        false,
    );
    set_claim(
        &mut short,
        "exp",
        (chrono::Utc::now().timestamp() + 3).into(),
    )?;
    crate::run_staging_expiry_row(
        client,
        sign_assertion(&short, FIXTURE_KID)?,
        owner_assertion()?,
    )
    .await
}

/// A successful verification cannot extend a key source's zero cache lifetime.
pub async fn run_access_zero_age_conformance(
    client: &ConformanceClient,
    jwks_requests: &std::sync::atomic::AtomicU64,
) -> ConformanceResult<()> {
    let assertion = owner_assertion()?;
    let expected = AccessVerificationOutcome::Accepted(AccessCallerClass::Owner);
    equal(
        client
            .verify_access_assertion(assertion.clone(), "/admin/v1/machines/enroll")
            .await?,
        expected.clone(),
    )?;
    let before = jwks_requests.load(std::sync::atomic::Ordering::SeqCst);

    let verified = client
        .verify_access_assertion(assertion, "/admin/v1/machines/enroll")
        .await?;

    equal(verified, expected)?;
    let after = jwks_requests.load(std::sync::atomic::Ordering::SeqCst);
    check(
        after > before,
        "max-age=0 keys must be fetched again on the next verification",
    )?;
    println!("ACCESS max-age=0: accepted owner, JWKS requests {before} → {after}: passed");
    Ok(())
}

/// The retained 2048-bit, tampered and valid 1024-bit fixtures through real crypto.
pub async fn run_access_key_profile_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    #[derive(serde::Deserialize)]
    struct WeakAssertionFixture {
        assertion: String,
    }
    let weak: WeakAssertionFixture =
        serde_json::from_str(include_str!("../fixtures/access-weak-1024.json"))?;
    let owner = owner_assertion()?;
    let cases = [
        (
            owner.clone(),
            AccessVerificationOutcome::Accepted(AccessCallerClass::Owner),
        ),
        (
            flip_signature_byte(&owner)?,
            AccessVerificationOutcome::Refused(AccessRefusal::BadSignature),
        ),
        (
            weak.assertion,
            AccessVerificationOutcome::Refused(AccessRefusal::InvalidKey),
        ),
    ];
    for (assertion, expected) in cases {
        equal(
            client
                .verify_access_assertion(assertion, "/admin/v1/machines/enroll")
                .await?,
            expected,
        )?;
    }
    println!(
        "ACCESS key profile: valid 2048 accepted, tampered refused, valid 1024 refused: passed"
    );
    Ok(())
}

type RefusedAccessAssertion = (&'static str, &'static str, String, AccessRefusal);

pub(crate) fn refused_assertions(
    claims: serde_json::Value,
) -> ConformanceResult<Vec<RefusedAccessAssertion>> {
    let admin_path = "/admin/v1/machines/enroll";
    let mut cases = Vec::new();
    let mut expired = claims.clone();
    set_claim(&mut expired, "exp", serde_json::json!(0))?;
    cases.push((
        "expired",
        "/v1/session",
        sign_assertion(&expired, FIXTURE_KID)?,
        AccessRefusal::Expired,
    ));
    let mut future = claims.clone();
    set_claim(&mut future, "nbf", serde_json::json!(i64::MAX))?;
    cases.push((
        "not-yet-valid",
        "/v1/session",
        sign_assertion(&future, FIXTURE_KID)?,
        AccessRefusal::NotYetValid,
    ));
    cases.push((
        "session audience on admin",
        admin_path,
        sign_assertion(&claims, FIXTURE_KID)?,
        AccessRefusal::WrongAudience,
    ));
    let owner = fixture_claims(
        AccessCaller::Owner {
            email: fixture_policy().owner_identity,
        },
        true,
    );
    cases.push((
        "admin audience on session",
        "/v1/session",
        sign_assertion(&owner, FIXTURE_KID)?,
        AccessRefusal::WrongAudience,
    ));
    let mut wrong_issuer = claims.clone();
    set_claim(
        &mut wrong_issuer,
        "iss",
        serde_json::json!("https://wrong-team.cloudflareaccess.com"),
    )?;
    cases.push((
        "issuer",
        "/v1/session",
        sign_assertion(&wrong_issuer, FIXTURE_KID)?,
        AccessRefusal::WrongIssuer,
    ));
    cases.push((
        "bad signature",
        "/v1/session",
        flip_signature_byte(&sign_assertion(&claims, FIXTURE_KID)?)?,
        AccessRefusal::BadSignature,
    ));
    let mut machine_admin = claims;
    set_claim(
        &mut machine_admin,
        "aud",
        serde_json::json!([fixture_policy().admin_audience]),
    )?;
    cases.push((
        "service class on admin",
        admin_path,
        sign_assertion(&machine_admin, FIXTURE_KID)?,
        AccessRefusal::WrongCallerClass,
    ));
    let mut owner_session = owner.clone();
    set_claim(
        &mut owner_session,
        "aud",
        serde_json::json!([fixture_policy().session_audience]),
    )?;
    cases.push((
        "owner class on session",
        "/v1/session",
        sign_assertion(&owner_session, FIXTURE_KID)?,
        AccessRefusal::WrongCallerClass,
    ));
    let mut wrong_owner = owner;
    set_claim(
        &mut wrong_owner,
        "email",
        serde_json::json!("other@example.test"),
    )?;
    cases.push((
        "owner identity",
        admin_path,
        sign_assertion(&wrong_owner, FIXTURE_KID)?,
        AccessRefusal::WrongOwner,
    ));
    Ok(cases)
}

/// The conformance build submits one assertion. Staging feeds it real captured Access assertions; here
/// the fixture key signs them.
pub async fn run_access_verifier_control_conformance(
    client: &ConformanceClient,
) -> ConformanceResult<()> {
    let token = "verifier-control-machine";
    let admin_path = "/admin/v1/machines/enroll";
    let owner = owner_assertion()?;
    let machine = machine_assertion(token)?;
    let mut expired = fixture_claims(
        AccessCaller::Machine {
            client_id: access_client_id(token)?,
        },
        false,
    );
    set_claim(&mut expired, "exp", serde_json::json!(0))?;
    use AccessVerificationOutcome::{Accepted, Refused};
    let cases = [
        (
            owner.clone(),
            admin_path,
            Accepted(AccessCallerClass::Owner),
        ),
        (
            flip_signature_byte(&owner)?,
            admin_path,
            Refused(AccessRefusal::BadSignature),
        ),
        (
            owner.clone(),
            admin_path,
            Accepted(AccessCallerClass::Owner),
        ),
        (
            machine.clone(),
            "/v1/session",
            Accepted(AccessCallerClass::Machine),
        ),
        (machine, admin_path, Refused(AccessRefusal::WrongAudience)),
        (owner, "/v1/session", Refused(AccessRefusal::WrongAudience)),
        (
            sign_assertion(&expired, FIXTURE_KID)?,
            "/v1/session",
            Refused(AccessRefusal::Expired),
        ),
    ];
    for (assertion, path, expected) in cases {
        equal(
            client.verify_access_assertion(assertion, path).await?,
            expected,
        )?;
    }
    println!(
        "ACCESS verifier control: owner accepted, one flipped signature byte refused, owner accepted again, machine accepted, wrong audience both ways, expired: passed"
    );
    Ok(())
}

fn set_claim(
    claims: &mut serde_json::Value,
    name: &str,
    value: serde_json::Value,
) -> ConformanceResult<()> {
    claims
        .as_object_mut()
        .ok_or("fixture claims object")?
        .insert(name.into(), value);
    Ok(())
}
