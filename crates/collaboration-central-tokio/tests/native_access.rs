#![cfg(feature = "conformance")]
#![allow(clippy::panic_in_result_fn)]

use collaboration_central_conformance::{
    JwksFixtureServer, fixture_key_profile_jwks, fixture_policy, flip_signature_byte,
    owner_assertion,
};
use collaboration_central_core::{
    AccessCaller, AccessKeyCache, AccessRefusal, CentralApplication, CentralPlatform,
    KeyFetchFailure,
};
#[path = "native_http/platform.rs"]
mod support;
use std::sync::{Arc, atomic::Ordering};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
async fn the_core_application_uses_real_native_key_fetch_and_signature_verification() -> TestResult
{
    #[derive(serde::Deserialize)]
    struct WeakAssertionFixture {
        assertion: String,
    }
    let weak: WeakAssertionFixture = serde_json::from_str(include_str!(
        "../../collaboration-central-conformance/fixtures/access-weak-1024.json"
    ))?;
    let keys = JwksFixtureServer::start_with_keys(fixture_key_profile_jwks()?).await?;
    let (platform, _directory) = support::native_platform(&keys.url).await?;
    let application = CentralApplication::new(
        platform.clone(),
        fixture_policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    let owner = owner_assertion()?;
    let proof = async {
        assert_eq!(
            application
                .verify_access(Some(&owner), "/admin/v1/operations")
                .await?,
            AccessCaller::Owner {
                email: fixture_policy().owner_identity
            }
        );
        assert_eq!(
            application
                .verify_access(Some(&flip_signature_byte(&owner)?), "/admin/v1/operations")
                .await,
            Err(AccessRefusal::BadSignature)
        );
        assert_eq!(
            application
                .verify_access(Some(&weak.assertion), "/admin/v1/operations")
                .await,
            Err(AccessRefusal::InvalidKey)
        );
        assert_eq!(keys.requests.load(Ordering::SeqCst), 1);
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    platform.shutdown().await?;
    keys.shutdown().await?;
    proof
}

#[tokio::test]
async fn the_native_key_fetch_preserves_cache_metadata_and_reports_http_errors() -> TestResult {
    let keys = JwksFixtureServer::start().await?;
    let (healthy, _healthy_directory) = support::native_platform(&keys.url).await?;
    let response = healthy
        .fetch_access_keys()
        .await
        .map_err(|failure| format!("healthy JWKS fetch failed: {failure:?}"))?;
    assert_eq!(
        response.cache_control.as_deref(),
        Some("public, max-age=3600")
    );
    let mut source = reqwest::Url::parse(&keys.url)?;
    source.set_path("/missing-jwks");
    let (platform, _directory) = support::native_platform(source.as_str()).await?;

    let outcome = platform.fetch_access_keys().await;
    healthy.shutdown().await?;

    platform.shutdown().await?;
    keys.shutdown().await?;
    assert!(matches!(
        outcome,
        Err(KeyFetchFailure::BadResponse { status: 404 })
    ));
    Ok(())
}

#[tokio::test]
async fn the_core_application_refetches_zero_age_keys_through_the_native_capability() -> TestResult
{
    let keys = JwksFixtureServer::start_with_cache_control(
        collaboration_central_conformance::fixture_jwks()?,
        "public, max-age=0",
    )
    .await?;
    let (platform, _directory) = support::native_platform(&keys.url).await?;
    let application = CentralApplication::new(
        platform.clone(),
        fixture_policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    let owner = owner_assertion()?;

    let proof = async {
        let first = application
            .verify_access(Some(&owner), "/admin/v1/operations")
            .await?;
        let second = application
            .verify_access(Some(&owner), "/admin/v1/operations")
            .await?;

        let expected = AccessCaller::Owner {
            email: fixture_policy().owner_identity,
        };
        assert_eq!(first, expected);
        assert_eq!(second, expected);
        assert_eq!(keys.requests.load(Ordering::SeqCst), 2);
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    platform.shutdown().await?;
    keys.shutdown().await?;
    proof
}
