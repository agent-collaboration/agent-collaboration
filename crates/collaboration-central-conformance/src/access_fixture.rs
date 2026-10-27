use crate::ConformanceResult;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::*;
use ring::{
    rand::SystemRandom,
    rsa::{KeyPair, PublicKeyComponents},
    signature::RSA_PKCS1_SHA256,
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
pub const FIXTURE_KID: &str = "access-test-key";
pub fn access_client_id(machine_token: &str) -> ConformanceResult<AccessClientId> {
    use sha2::{Digest, Sha256};
    Ok(AccessClientId::new(format!(
        "test-client-{:x}.access",
        Sha256::digest(machine_token.as_bytes())
    ))?)
}
pub fn fixture_policy() -> AccessPolicy {
    AccessPolicy {
        issuer: "https://conformance-test.cloudflareaccess.com".into(),
        jwks_url: "https://conformance-test.cloudflareaccess.com/cdn-cgi/access/certs".into(),
        session_audience: "test-session-aud".into(),
        admin_audience: "test-admin-aud".into(),
        owner_identity: "owner@example.test".into(),
    }
}
fn private_key() -> ConformanceResult<KeyPair> {
    KeyPair::from_pkcs8(include_bytes!("../fixtures/access-test-only-rsa.pk8"))
        .map_err(|_| "fixture PKCS8 key rejected".into())
}
pub fn fixture_jwks() -> ConformanceResult<AccessJwks> {
    let key = private_key()?;
    let public: PublicKeyComponents<Vec<u8>> = key.public().into();
    Ok(AccessJwks {
        keys: vec![AccessJwk {
            kid: FIXTURE_KID.into(),
            kty: "RSA".into(),
            alg: Some("RS256".into()),
            key_use: Some("sig".into()),
            n: URL_SAFE_NO_PAD.encode(public.n),
            e: URL_SAFE_NO_PAD.encode(public.e),
        }],
    })
}

pub fn fixture_key_profile_jwks() -> ConformanceResult<AccessJwks> {
    #[derive(serde::Deserialize)]
    struct WeakKeyFixture {
        key: AccessJwk,
    }
    let weak: WeakKeyFixture =
        serde_json::from_str(include_str!("../fixtures/access-weak-1024.json"))?;
    let mut keys = fixture_jwks()?;
    keys.keys.push(weak.key);
    Ok(keys)
}
pub fn fixture_claims(caller: AccessCaller, admin: bool) -> serde_json::Value {
    let policy = fixture_policy();
    let now = chrono::Utc::now().timestamp();
    let issuer = policy.issuer();
    let audience = if admin {
        policy.admin_audience
    } else {
        policy.session_audience
    };
    match caller {
        AccessCaller::Machine { client_id } => {
            // The service-token assertion Access really issues (observed on
            // staging): a single-string `aud` and no `nbf`.
            serde_json::json!({"iss":issuer,"aud":audience,"exp":now+86400*60,"iat":now,"type":"app","sub":"","common_name":client_id})
        }
        AccessCaller::Owner { email } => {
            serde_json::json!({"iss":issuer,"aud":[audience],"exp":now+86400*60,"nbf":now-86400*60,"type":"app","sub":"test-google-owner-subject","email":email})
        }
    }
}

pub fn sign_assertion(claims: &serde_json::Value, kid: &str) -> ConformanceResult<String> {
    let header = URL_SAFE_NO_PAD.encode(serde_json::to_vec(
        &serde_json::json!({"alg":"RS256","kid":kid,"typ":"JWT"}),
    )?);
    let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?);
    let signed = format!("{header}.{payload}");
    let key = private_key()?;
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signed.as_bytes(),
        &mut signature,
    )
    .map_err(|_| "fixture RSA signing failed")?;
    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
/// The same assertion with one signature byte changed: every signed header
/// and claim byte, and the key ID, stay as issued.
pub fn flip_signature_byte(assertion: &str) -> ConformanceResult<String> {
    let (signed, signature) = assertion.rsplit_once('.').ok_or("assertion signature")?;
    let mut signature = URL_SAFE_NO_PAD.decode(signature)?;
    let first = signature.first_mut().ok_or("empty assertion signature")?;
    *first ^= 1;
    Ok(format!("{signed}.{}", URL_SAFE_NO_PAD.encode(signature)))
}
pub fn owner_assertion() -> ConformanceResult<String> {
    sign_assertion(
        &fixture_claims(
            AccessCaller::Owner {
                email: fixture_policy().owner_identity,
            },
            true,
        ),
        FIXTURE_KID,
    )
}
pub fn machine_assertion(token: &str) -> ConformanceResult<String> {
    sign_assertion(
        &fixture_claims(
            AccessCaller::Machine {
                client_id: access_client_id(token)?,
            },
            false,
        ),
        FIXTURE_KID,
    )
}
pub struct JwksFixtureServer {
    pub url: String,
    pub requests: Arc<AtomicU64>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
impl JwksFixtureServer {
    pub async fn start() -> ConformanceResult<Self> {
        Self::start_with_keys(fixture_jwks()?).await
    }
    pub async fn start_with_keys(keys: AccessJwks) -> ConformanceResult<Self> {
        Self::start_with_cache_control(keys, "public, max-age=3600").await
    }
    pub async fn start_with_cache_control(
        keys: AccessJwks,
        cache_control: &str,
    ) -> ConformanceResult<Self> {
        let requests = Arc::new(AtomicU64::new(0));
        let state = (keys, requests.clone(), cache_control.to_owned());
        let router = axum::Router::new()
            .route(
                "/cdn-cgi/access/certs",
                axum::routing::get(
                    |axum::extract::State((keys, count, cache_control)): axum::extract::State<
                        (AccessJwks, Arc<AtomicU64>, String),
                    >| async move {
                        count.fetch_add(1, Ordering::SeqCst);
                        ([("cache-control", cache_control)], axum::Json(keys))
                    },
                ),
            )
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/cdn-cgi/access/certs", listener.local_addr()?);
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
            requests,
            stop,
            task,
        })
    }
    pub async fn shutdown(self) -> ConformanceResult<()> {
        let _ = self.stop.send(());
        self.task.await??;
        Ok(())
    }
}
