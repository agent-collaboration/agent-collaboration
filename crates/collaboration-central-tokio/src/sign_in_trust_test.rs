use super::NativeSignInTrust;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    SignInTrustSettings, organization_authority::FleetOrganizationStatementRefusal,
};
use fleet_identity_protocol::OrganizationId;
use ring::signature::{Ed25519KeyPair, KeyPair};
use router_clock::ManualClock;
use runtime_primitives::SyncTime;
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000b2a1";
const OTHER_ORGANIZATION: &str = "019f0000-0000-7000-8000-00000000b2b1";
const NOW_SECONDS: i64 = 1_900_000_000;

fn settings(jwks_url: String) -> SignInTrustSettings {
    SignInTrustSettings {
        issuer: "https://sign-in.example.invalid".into(),
        audience: "https://central.example.invalid/identity".into(),
        admin_audience: "https://central.example.invalid/admin".into(),
        public_origin: "https://central.example.invalid".into(),
        jwks_url,
        service_id: None,
    }
}

fn statement(organization: &str) -> TestResult<String> {
    let header = json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-a"});
    let claims = json!({
        "purpose":"fleet-organization-stamp","version":1,
        "iss":"https://sign-in.example.invalid",
        "aud":"https://central.example.invalid/identity",
        "sub":organization,"organization_id":organization,
        "iat":NOW_SECONDS - 60,"exp":NOW_SECONDS + 600
    });
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims)?)
    );
    // Fixed public test seed, never a deployment signing key.
    let signer =
        Ed25519KeyPair::from_seed_unchecked(&[7; 32]).map_err(|_| "fixture Ed25519 key")?;
    let signature = signer.sign(signed.as_bytes());
    Ok(format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

struct PublishedKeys {
    url: String,
    retired: Arc<AtomicBool>,
    fetches: Arc<AtomicUsize>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}

impl PublishedKeys {
    async fn start() -> TestResult<Self> {
        let signer =
            Ed25519KeyPair::from_seed_unchecked(&[7; 32]).map_err(|_| "fixture Ed25519 key")?;
        let public_key = URL_SAFE_NO_PAD.encode(signer.public_key().as_ref());
        let retired = Arc::new(AtomicBool::new(false));
        let fetches = Arc::new(AtomicUsize::new(0));
        let published_retired = retired.clone();
        let published_fetches = fetches.clone();
        let router = axum::Router::new().route(
            "/jwks",
            axum::routing::get(move || {
                let public_key = public_key.clone();
                let retired = published_retired.load(Ordering::SeqCst);
                published_fetches.fetch_add(1, Ordering::SeqCst);
                async move {
                    let keys = if retired {
                        json!({"keys":[]})
                    } else {
                        json!({"keys":[{"kid":"stamp-a","kty":"OKP","crv":"Ed25519",
                            "alg":"EdDSA","x":public_key}]})
                    };
                    // A larger provider max-age must still obey core's 300s cap.
                    (
                        [("cache-control", "public, max-age=3600")],
                        axum::Json(keys),
                    )
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/jwks", listener.local_addr()?);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            url,
            retired,
            fetches,
            stop,
            task,
        })
    }

    async fn shutdown(self) -> TestResult {
        let _ = self.stop.send(());
        self.task.await??;
        Ok(())
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare real fetched keys and injected-clock expiry while fixture errors propagate."
)]
#[tokio::test]
async fn native_factory_preserves_org_binding_refresh_bound_and_current_clock() -> TestResult {
    // Arrange: actual HTTP publication and ring verification, one injected clock.
    let keys = PublishedKeys::start().await?;
    let clock = Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(
        NOW_SECONDS * 1000,
    ))?);
    let trust = NativeSignInTrust::new(settings(keys.url.clone()))?;
    let organization = OrganizationId::try_from(ORGANIZATION.to_owned())?;
    let verifier = trust
        .for_organization(organization, clock.clone())
        .map_err(|_| "fixture trust")?;
    let stamp = statement(ORGANIZATION)?;
    let other_stamp = statement(OTHER_ORGANIZATION)?;

    // Act: the cache is shared by separately constructed org verifiers.
    assert!(verifier.verify_statement(&stamp).await.is_ok());
    let foreign = verifier.verify_statement(&other_stamp).await;
    let other = trust
        .for_organization(
            OrganizationId::try_from(OTHER_ORGANIZATION.to_owned())?,
            clock.clone(),
        )
        .map_err(|_| "fixture other trust")?;
    assert!(other.verify_statement(&other_stamp).await.is_ok());
    let first_fetches = keys.fetches.load(Ordering::SeqCst);
    keys.retired.store(true, Ordering::SeqCst);
    clock.advance(Duration::from_millis(299_999))?;
    let inside_bound = verifier.verify_statement(&stamp).await;
    let inside_fetches = keys.fetches.load(Ordering::SeqCst);
    clock.advance(Duration::from_millis(1))?;
    let after_bound = verifier.verify_statement(&stamp).await;
    let boundary_fetches = keys.fetches.load(Ordering::SeqCst);

    // Assert: max-age cannot retain a retired key beyond core's cap.
    assert!(matches!(
        foreign,
        Err(FleetOrganizationStatementRefusal::OrganizationMismatch)
    ));
    assert!(inside_bound.is_ok());
    assert!(matches!(
        after_bound,
        Err(FleetOrganizationStatementRefusal::BadSignature)
    ));
    assert_eq!((first_fetches, inside_fetches, boundary_fetches), (1, 1, 2));

    // Restore publication, then cross statement expiry using that same clock.
    keys.retired.store(false, Ordering::SeqCst);
    assert!(verifier.verify_statement(&stamp).await.is_ok());
    clock.advance(Duration::from_secs(300))?;
    assert!(matches!(
        verifier.verify_statement(&stamp).await,
        Err(FleetOrganizationStatementRefusal::Expired)
    ));
    keys.shutdown().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The constructor must refuse invalid issuer configuration before key fetching."
)]
#[tokio::test]
async fn native_factory_refuses_invalid_trust_configuration() -> TestResult {
    let mut invalid = settings("http://127.0.0.1:1/jwks".into());
    invalid.issuer.clear();
    let trust = NativeSignInTrust::new(invalid)?;
    let clock = Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(
        NOW_SECONDS * 1000,
    ))?);
    let verifier =
        trust.for_organization(OrganizationId::try_from(ORGANIZATION.to_owned())?, clock);
    assert!(matches!(
        verifier,
        Err(FleetOrganizationStatementRefusal::InvalidConfiguration)
    ));
    Ok(())
}
