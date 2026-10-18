use collaboration_central_core::*;
use js_sys::{Array, Object, futures::JsFuture};
use serde::Serialize;
use worker::{
    Cache, Env, Fetch, Response,
    wasm_bindgen::{JsCast, JsValue},
};

#[derive(Serialize)]
struct ImportAlgorithm {
    name: &'static str,
    hash: &'static str,
}
struct WebCryptoSignaturePort;
impl AccessSignaturePort for WebCryptoSignaturePort {
    async fn verify<'a>(
        &'a self,
        key: &'a AccessJwk,
        assertion: &'a ParsedAccessAssertion,
    ) -> Result<SignatureValidity, AccessRefusal> {
        // The Workers SDK uses this supported global-scope binding too.
        let global: web_sys::WorkerGlobalScope = js_sys::global().unchecked_into();
        let crypto = global
            .crypto()
            .map_err(|_| AccessRefusal::BadSignature)?
            .subtle();
        let object = |value: JsValue| {
            value
                .dyn_into::<Object>()
                .map_err(|_| AccessRefusal::InvalidKey)
        };
        let jwk =
            object(serde_wasm_bindgen::to_value(key).map_err(|_| AccessRefusal::InvalidKey)?)?;
        let algorithm = object(
            serde_wasm_bindgen::to_value(&ImportAlgorithm {
                name: "RSASSA-PKCS1-v1_5",
                hash: "SHA-256",
            })
            .map_err(|_| AccessRefusal::InvalidKey)?,
        )?;
        let usages = Array::new();
        usages.push(&JsValue::from_str("verify"));
        let imported = JsFuture::from(
            crypto
                .import_key_with_object("jwk", &jwk, &algorithm, false, &usages)
                .map_err(|_| AccessRefusal::InvalidKey)?,
        )
        .await
        .map_err(|_| AccessRefusal::InvalidKey)?;
        let key = imported
            .dyn_into::<web_sys::CryptoKey>()
            .map_err(|_| AccessRefusal::InvalidKey)?;
        let result = JsFuture::from(
            crypto
                .verify_with_str_and_u8_array_and_u8_array(
                    "RSASSA-PKCS1-v1_5",
                    &key,
                    assertion.signature(),
                    assertion.signed_bytes(),
                )
                .map_err(|_| AccessRefusal::BadSignature)?,
        )
        .await
        .map_err(|_| AccessRefusal::BadSignature)?;
        match result.as_bool() {
            Some(true) => Ok(SignatureValidity::Valid),
            Some(false) => Ok(SignatureValidity::Invalid),
            None => Err(AccessRefusal::BadSignature),
        }
    }
}
pub(crate) fn policy(env: &Env) -> worker::Result<AccessPolicy> {
    let policy = AccessPolicy {
        team_domain: env.var("ACCESS_TEAM_DOMAIN")?.to_string(),
        session_audience: env.var("ACCESS_SESSION_AUD")?.to_string(),
        admin_audience: env.var("ACCESS_ADMIN_AUD")?.to_string(),
        owner_identity: env.var("ACCESS_OWNER_IDENTITY")?.to_string(),
    };
    policy
        .validate()
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    Ok(policy)
}
/// Where the signing keys for one verification came from, for the
/// verification log line.
#[derive(Clone, Copy)]
enum KeySource {
    Cached,
    Fetched,
    Refetched,
}
impl KeySource {
    const fn label(self) -> &'static str {
        match self {
            Self::Cached => "cached",
            Self::Fetched => "fetched",
            Self::Refetched => "refetched",
        }
    }
}
async fn keys(source: &str, refresh: bool) -> Result<(AccessJwks, KeySource), AccessRefusal> {
    let cache = Cache::default();
    if !refresh
        && let Some(mut response) = cache
            .get(source.to_owned(), false)
            .await
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?
    {
        let keys = serde_json::from_str(
            &response
                .text()
                .await
                .map_err(|_| AccessRefusal::KeySourceUnavailable)?,
        )
        .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
        return Ok((keys, KeySource::Cached));
    }
    let mut response = Fetch::Url(
        source
            .parse()
            .map_err(|_| AccessRefusal::KeySourceUnavailable)?,
    )
    .send()
    .await
    .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    if response.status_code() != 200 {
        return Err(AccessRefusal::KeySourceUnavailable);
    }
    let content = response
        .text()
        .await
        .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    let keys = serde_json::from_str(&content).map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    let mut cached = Response::ok(content).map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    cached
        .headers_mut()
        .set("cache-control", "public, max-age=3600")
        .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    cache
        .put(source.to_owned(), cached)
        .await
        .map_err(|_| AccessRefusal::KeySourceUnavailable)?;
    Ok((
        keys,
        if refresh {
            KeySource::Refetched
        } else {
            KeySource::Fetched
        },
    ))
}
/// Which check a verification log line belongs to: the edge's gate on the
/// request itself, or the conformance control's check of an assertion
/// submitted inside that same request.
#[derive(Clone, Copy)]
pub(crate) enum VerificationLayer {
    Edge,
    #[cfg(feature = "conformance")]
    ControlInner,
}
impl VerificationLayer {
    const fn label(self) -> &'static str {
        match self {
            Self::Edge => "edge",
            #[cfg(feature = "conformance")]
            Self::ControlInner => "control-inner",
        }
    }
}

/// Benign labels that tie one request's log lines together: a random label
/// made for this invocation, and the platform's ray ID when there is one.
pub(crate) struct InvocationLabels {
    invocation: String,
    ray: String,
}
impl InvocationLabels {
    pub(crate) fn for_request(request: &worker::Request) -> worker::Result<Self> {
        Ok(Self {
            invocation: uuid::Uuid::now_v7().to_string(),
            ray: request.headers().get("cf-ray")?.unwrap_or_default(),
        })
    }
}

/// Verifies one Access assertion and logs one secret-free line: the
/// invocation label, the layer, the ray ID, the route class, the public key
/// ID, where the keys came from and the outcome. Never the assertion, its
/// claims or identity.
pub(crate) async fn verify(
    env: &Env,
    header: Option<&str>,
    path: &str,
    labels: &InvocationLabels,
    layer: VerificationLayer,
) -> Result<AccessCaller, AccessRefusal> {
    let mut kid = None;
    let mut key_source = None;
    let verified = verify_assertion(env, header, path, &mut kid, &mut key_source).await;
    let outcome = match &verified {
        Ok(AccessCaller::Machine { .. }) => "accepted machine".to_owned(),
        Ok(AccessCaller::Owner { .. }) => "accepted owner".to_owned(),
        Err(refusal) => format!("refused {refusal:?}"),
    };
    worker::console_log!(
        "central access: invocation={} layer={} ray={} route={} kid={} keys={} outcome={}",
        labels.invocation,
        layer.label(),
        log_safe(&labels.ray),
        route_class(path),
        kid.as_deref().map_or("-", log_safe),
        key_source.map_or("-", KeySource::label),
        outcome
    );
    verified
}
async fn verify_assertion(
    env: &Env,
    header: Option<&str>,
    path: &str,
    kid: &mut Option<String>,
    key_source: &mut Option<KeySource>,
) -> Result<AccessCaller, AccessRefusal> {
    let policy = policy(env).map_err(|_| AccessRefusal::InvalidCaller)?;
    let assertion =
        ParsedAccessAssertion::parse(header, &policy, path, RegistryTime(chrono::Utc::now()))?;
    *kid = Some(assertion.kid().to_owned());
    let source = policy.certs_url();
    let (mut keys, mut fetched) = keys(&source, false).await?;
    if matches!(keys.key(assertion.kid()), Err(AccessRefusal::UnknownKey)) {
        (keys, fetched) = crate::access_verification::keys(&source, true).await?;
    }
    *key_source = Some(fetched);
    let validity = WebCryptoSignaturePort
        .verify(keys.key(assertion.kid())?, &assertion)
        .await?;
    let caller = assertion.verified(validity)?;
    authorize_access_route(&policy, path, &caller)?;
    Ok(caller)
}
fn route_class(path: &str) -> &'static str {
    if path == "/v1/session" {
        "session"
    } else if path == "/admin/v1/conformance" {
        "control"
    } else if path.starts_with("/admin/v1/") {
        "admin"
    } else {
        "other"
    }
}
/// Key IDs and ray IDs are public, but both arrive in caller-controlled bytes;
/// only short identifier characters reach the log.
fn log_safe(value: &str) -> &str {
    if value.is_empty() {
        "-"
    } else if value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        value
    } else {
        "unprintable"
    }
}
