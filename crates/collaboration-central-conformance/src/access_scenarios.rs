use crate::{
    AccessCallerClass, AccessVerificationOutcome, ConformanceClient, ConformanceResult,
    access_fixture::*,
    holder_fixtures::{check, equal},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use reqwest_websocket::RequestBuilderExt;
pub async fn run_access_conformance(
    client: &ConformanceClient,
    jwks_requests: &std::sync::atomic::AtomicU64,
) -> ConformanceResult<()> {
    let token = "access-suite-machine";
    let other = "access-suite-other-machine";
    let installation = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
    let other_installation = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
    for (installation, token) in [(installation.clone(), token), (other_installation, other)] {
        use sha2::{Digest, Sha256};
        equal(
            client
                .admin(AdminRequest::EnrollMachine {
                    label: "test machine".into(),
                    installation,
                    access_client_id: access_client_id(token)?,
                    credential_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
                    expires_at: chrono::Utc::now() + chrono::TimeDelta::days(30),
                })
                .await?,
            AdminResult::MachineEnrolled,
        )?;
    }
    let mut socket = client.connect(token).await?;
    let session_id = SessionId::new(uuid::Uuid::now_v7().to_string())?;
    socket
        .send(&MachineFrame::Hello {
            installation: installation.clone(),
            session_id: session_id.clone(),
            schema_support: SchemaSupport { max_version: 10 },
        })
        .await?;
    check(
        matches!(socket.receive().await?,CentralFrame::Welcome { session_id:actual,.. } if actual==session_id),
        "valid signed machine and matching credential upgrade",
    )?;
    let claims = fixture_claims(
        AccessCaller::Machine {
            client_id: access_client_id(token)?,
        },
        false,
    );
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
    let mut machine_admin = claims.clone();
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
    let mut wrong_owner = owner.clone();
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
    for (label, path, assertion, reason) in cases {
        let response = client
            .client
            .get(format!("{}{path}", client.base_url))
            .bearer_auth(token)
            .header("Cf-Access-Jwt-Assertion", assertion)
            .send()
            .await?;
        equal(response.status(), reqwest::StatusCode::UNAUTHORIZED)?;
        equal(response.json::<AccessRefusal>().await?, reason)?;
        println!("ACCESS {label}: refused");
    }
    let before = jwks_requests.load(std::sync::atomic::Ordering::SeqCst);
    let response = client
        .client
        .get(format!("{}/v1/session", client.base_url))
        .bearer_auth(token)
        .header(
            "Cf-Access-Jwt-Assertion",
            sign_assertion(&claims, "unknown-access-key")?,
        )
        .send()
        .await?;
    equal(
        response.json::<AccessRefusal>().await?,
        AccessRefusal::UnknownKey,
    )?;
    equal(
        jwks_requests.load(std::sync::atomic::Ordering::SeqCst),
        before + 1,
    )?;
    let missing = client
        .client
        .get(format!("{}/v1/session", client.base_url))
        .bearer_auth(token)
        .upgrade()
        .send()
        .await?;
    equal(missing.status(), reqwest::StatusCode::UNAUTHORIZED)?;
    equal(
        missing.into_inner().json::<AccessRefusal>().await?,
        AccessRefusal::MissingAssertion,
    )?;
    let mismatch = client
        .client
        .get(format!("{}/v1/session", client.base_url))
        .bearer_auth(other)
        .header("Cf-Access-Jwt-Assertion", machine_assertion(token)?)
        .upgrade()
        .send()
        .await?;
    equal(mismatch.status(), reqwest::StatusCode::UNAUTHORIZED)?;
    equal(
        mismatch.into_inner().json::<CredentialRejection>().await?,
        CredentialRejection::NotMachine,
    )?;
    equal(
        client
            .admin(AdminRequest::RevokeMachine { installation })
            .await?,
        AdminResult::MachineRevoked,
    )?;
    equal(
        socket.receive().await?,
        CentralFrame::Closed {
            reason: SessionCloseReason::CredentialRejected(CredentialRejection::Revoked),
        },
    )?;
    check(
        client.connect(token).await.is_err(),
        "revoked installation cannot reopen",
    )?;
    let unknown = client
        .client
        .get(format!("{}/not-a-central-route", client.base_url))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?;
    equal(
        unknown.json::<AccessRefusal>().await?,
        AccessRefusal::RouteRefused,
    )?;
    println!(
        "ACCESS valid owner/machine, 9 claim/signature/class refusals, unknown kid refreshed exactly once, missing header, cross-installation binding, revoked live session closed and re-upgrade denied, unknown path: passed"
    );
    Ok(())
}

/// The conformance build's verifier control runs the edge's own verifier on a
/// submitted assertion. Staging feeds it real captured Access assertions; here
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
