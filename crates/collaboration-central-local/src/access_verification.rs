use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::*;
use ring::{rsa::PublicKeyComponents, signature::RSA_PKCS1_2048_8192_SHA256};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

struct NativeSignaturePort;
impl AccessSignaturePort for NativeSignaturePort {
    fn verify<'a>(
        &'a self,
        key: &'a AccessJwk,
        assertion: &'a ParsedAccessAssertion,
    ) -> impl std::future::Future<Output = Result<SignatureValidity, AccessRefusal>> + 'a {
        std::future::ready((|| {
            let n = URL_SAFE_NO_PAD
                .decode(&key.n)
                .map_err(|_| AccessRefusal::InvalidKey)?;
            let e = URL_SAFE_NO_PAD
                .decode(&key.e)
                .map_err(|_| AccessRefusal::InvalidKey)?;
            let n = unsigned_component(&n)?;
            let e = unsigned_component(&e)?;
            let first = n.first().ok_or(AccessRefusal::InvalidKey)?;
            let bits = n.len() * 8 - first.leading_zeros() as usize;
            // Preserve the previous verifier's 4096-bit upper bound; the
            // released policy raises its minimum to 2048 bits.
            if !(2048..=4096).contains(&bits) || n.last().is_none_or(|byte| byte % 2 == 0) {
                return Err(AccessRefusal::InvalidKey);
            }
            let exponent = e
                .iter()
                .try_fold(0_u64, |value, byte| {
                    value.checked_mul(256)?.checked_add(u64::from(*byte))
                })
                .ok_or(AccessRefusal::InvalidKey)?;
            if !(3..=(1_u64 << 33) - 1).contains(&exponent) || exponent % 2 == 0 {
                return Err(AccessRefusal::InvalidKey);
            }
            let public_key = PublicKeyComponents { n, e };
            Ok(
                if public_key
                    .verify(
                        &RSA_PKCS1_2048_8192_SHA256,
                        assertion.signed_bytes(),
                        assertion.signature(),
                    )
                    .is_ok()
                {
                    SignatureValidity::Valid
                } else {
                    SignatureValidity::Invalid
                },
            )
        })())
    }
}
fn unsigned_component(bytes: &[u8]) -> Result<&[u8], AccessRefusal> {
    let start = bytes
        .iter()
        .position(|byte| *byte != 0)
        .ok_or(AccessRefusal::InvalidKey)?;
    bytes.get(start..).ok_or(AccessRefusal::InvalidKey)
}
#[derive(Clone)]
pub(crate) struct AccessVerification {
    pub policy: AccessPolicy,
    source: String,
    client: reqwest::Client,
    cache: Arc<Mutex<Option<(AccessJwks, Instant)>>>,
}
impl AccessVerification {
    pub fn new(policy: AccessPolicy, source: String) -> crate::LocalResult<Self> {
        policy
            .validate()
            .map_err(|error| crate::LocalError::Transport(error.to_string()))?;
        let source_url = reqwest::Url::parse(&source)
            .map_err(|error| crate::LocalError::Transport(error.to_string()))?;
        if !matches!(source_url.scheme(), "http" | "https") {
            return Err("JWKS source must be HTTP(S)".into());
        }
        Ok(Self {
            policy,
            source,
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|error| crate::LocalError::Transport(error.to_string()))?,
            cache: Arc::new(Mutex::new(None)),
        })
    }
    async fn keys(&self, refresh: bool) -> Result<AccessJwks, AccessRefusal> {
        let mut cache = self.cache.lock().await;
        if !refresh
            && let Some((keys, expires)) = cache.as_ref()
            && Instant::now() < *expires
        {
            return Ok(keys.clone());
        }
        let response = self
            .client
            .get(&self.source)
            .send()
            .await
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?
            .error_for_status()
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        let max_age = response
            .headers()
            .get("cache-control")
            .and_then(|header| header.to_str().ok())
            .and_then(|header| {
                header
                    .split(',')
                    .find_map(|part| part.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
            })
            .unwrap_or(3600);
        let keys = response
            .json::<AccessJwks>()
            .await
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        *cache = Some((
            keys.clone(),
            Instant::now() + Duration::from_secs(max_age.min(3600)),
        ));
        Ok(keys)
    }
    pub async fn verify(
        &self,
        header: Option<&str>,
        path: &str,
        now: RegistryTime,
    ) -> Result<AccessCaller, AccessRefusal> {
        let assertion = ParsedAccessAssertion::parse(header, &self.policy, path, now)?;
        let mut keys = self.keys(false).await?;
        if matches!(keys.key(assertion.kid()), Err(AccessRefusal::UnknownKey)) {
            keys = self.keys(true).await?;
        }
        let validity = NativeSignaturePort
            .verify(keys.key(assertion.kid())?, &assertion)
            .await?;
        let caller = assertion.verified(validity)?;
        authorize_access_route(&self.policy, path, &caller)?;
        Ok(caller)
    }
}
