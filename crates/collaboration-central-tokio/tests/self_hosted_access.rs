#![allow(clippy::panic_in_result_fn)]
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration, Utc};
use collaboration_central_conformance::JwksFixtureServer;
use collaboration_central_core::{
    AccessCaller, AccessClientId, AccessKeyCache, AccessPolicy, AccessRefusal, CentralApplication,
};
use collaboration_central_tokio::SelfHostedIssuer;
use std::sync::Arc;
#[path = "native_http/platform.rs"]
mod support;
type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

fn policy() -> AccessPolicy {
    AccessPolicy {
        issuer: "http://127.0.0.1:19843".into(),
        jwks_url: "http://127.0.0.1:19843/.well-known/jwks.json".into(),
        session_audience: "self-hosted-session".into(),
        admin_audience: "self-hosted-admin".into(),
        owner_identity: "local-owner".into(),
    }
}
fn issuer(
    policy: AccessPolicy,
) -> Result<SelfHostedIssuer, collaboration_central_tokio::IssuerError> {
    SelfHostedIssuer::from_pkcs8(
        include_bytes!("../../collaboration-central-conformance/fixtures/access-test-only-rsa.pk8"),
        policy,
    )
}
fn owner() -> AccessCaller {
    AccessCaller::Owner {
        email: "local-owner".into(),
    }
}

async fn check(
    assertion: &str,
    configured: AccessPolicy,
    route: &str,
    expected: Result<AccessCaller, AccessRefusal>,
) -> TestResult {
    let keys = JwksFixtureServer::start_with_keys(issuer(policy())?.jwks()).await?;
    let (platform, _directory) = support::native_platform(&keys.url).await?;
    let application = CentralApplication::new(
        platform.clone(),
        configured,
        Arc::new(AccessKeyCache::default()),
    )?;
    let observed = application.verify_access(Some(assertion), route).await;
    platform.shutdown().await?;
    keys.shutdown().await?;
    assert_eq!(observed, expected);
    Ok(())
}

#[tokio::test]
async fn self_hosted_owner_and_machine_claims_pass_real_native_crypto() -> TestResult {
    let now = Utc::now();
    let owner_token = issuer(policy())?.mint(&owner(), now, now + Duration::minutes(15))?;
    check(&owner_token, policy(), "/admin/v1/copies", Ok(owner())).await?;
    let machine = AccessCaller::Machine {
        client_id: AccessClientId::new("local-machine")?,
    };
    let token = issuer(policy())?.mint(&machine, now, now + Duration::days(365))?;
    check(&token, policy(), "/v1/session", Ok(machine)).await
}
#[tokio::test]
async fn self_hosted_wrong_issuer_is_refused() -> TestResult {
    let mut different = policy();
    different.issuer = "http://127.0.0.1:19844".into();
    let now = Utc::now();
    let token = issuer(different)?.mint(&owner(), now, now + Duration::minutes(15))?;
    check(
        &token,
        policy(),
        "/admin/v1/copies",
        Err(AccessRefusal::WrongIssuer),
    )
    .await
}
#[tokio::test]
async fn self_hosted_admin_audience_cannot_open_a_machine_session() -> TestResult {
    let now = Utc::now();
    let token = issuer(policy())?.mint(&owner(), now, now + Duration::minutes(15))?;
    check(
        &token,
        policy(),
        "/v1/session",
        Err(AccessRefusal::WrongAudience),
    )
    .await
}
#[tokio::test]
async fn self_hosted_expired_assertion_is_refused() -> TestResult {
    let now = Utc::now() - Duration::hours(2);
    let token = issuer(policy())?.mint(&owner(), now, now + Duration::minutes(15))?;
    check(
        &token,
        policy(),
        "/admin/v1/copies",
        Err(AccessRefusal::Expired),
    )
    .await
}
#[tokio::test]
async fn self_hosted_unknown_key_is_refused() -> TestResult {
    let now = Utc::now();
    let token = issuer(policy())?.mint(&owner(), now, now + Duration::minutes(15))?;
    let (_, body) = token.split_once('.').ok_or("token header")?;
    let unknown = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"unknown-key","typ":"JWT"}"#),
        body
    );
    check(
        &unknown,
        policy(),
        "/admin/v1/copies",
        Err(AccessRefusal::UnknownKey),
    )
    .await
}
#[tokio::test]
async fn self_hosted_owner_must_match_after_signature_verification() -> TestResult {
    let now = Utc::now();
    let token = issuer(policy())?.mint(&owner(), now, now + Duration::minutes(15))?;
    let mut different = policy();
    different.owner_identity = "other-owner".into();
    check(
        &token,
        different,
        "/admin/v1/copies",
        Err(AccessRefusal::WrongOwner),
    )
    .await
}
#[tokio::test]
async fn generated_key_roundtrips_through_pkcs8_and_issues_verifiable_rs256() -> TestResult {
    let bytes = tokio::task::spawn_blocking(SelfHostedIssuer::generate_signing_key).await??;
    let issuer = SelfHostedIssuer::from_pkcs8(&bytes, policy())?;
    let keys = issuer.jwks();
    assert_eq!(URL_SAFE_NO_PAD.decode(&keys.keys[0].n)?.len(), 256);
    let server = JwksFixtureServer::start_with_keys(keys).await?;
    let (platform, _directory) = support::native_platform(&server.url).await?;
    let app = CentralApplication::new(
        platform.clone(),
        policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    let now = Utc::now();
    let token = issuer.mint(&owner(), now, now + Duration::minutes(15))?;
    let observed = app.verify_access(Some(&token), "/admin/v1/copies").await;
    platform.shutdown().await?;
    server.shutdown().await?;
    assert_eq!(observed, Ok(owner()));
    Ok(())
}
