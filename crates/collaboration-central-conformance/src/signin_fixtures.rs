//! Real sign-in signatures and loopback key publication for both host suites.
//! Every business time is supplied by the scenario's injected clock.
use crate::ConformanceResult;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
mod person_requests;
pub use person_requests::{
    CREATE, LOOKUP, MEMBER, MEMBER_SUBJECT, ORGANIZATION, OWNER, OWNER_SUBJECT, POLICY, Person,
    ProofKey, RECORDS, SESSION, canonical, create_body, create_intent, fresh_request_id,
    lookup_intent, records_intent, sha256_url,
};

pub const ISSUER: &str = "https://sign-in.example.invalid";
pub const AUDIENCE: &str = "https://central.example.invalid/identity";
pub const ADMIN_AUDIENCE: &str = "https://central.example.invalid/admin";
pub const PUBLIC_ORIGIN: &str = "https://central.example.invalid";
/// The historical B2 assertion time; scenarios may supply another sample.
pub const NOW_SECONDS: i64 = 1_900_000_000;
/// 120 days, the owner's statement lifetime.
pub const LIFETIME_SECONDS: i64 = 10_368_000;

pub fn signing_key(seed: u8) -> ConformanceResult<Ed25519KeyPair> {
    // Public test fixture only: never a deployment signing key.
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).map_err(|_| "fixture seed".into())
}

/// The keys the fixture sign-in service publishes: `stamp-a` (seed 7) and
/// `stamp-b` (seed 8). Seed 9 signs with an unpublished key.
pub fn published_keys() -> ConformanceResult<Value> {
    let key = |kid: &str, seed: u8| -> ConformanceResult<Value> {
        Ok(json!({"kid":kid,"kty":"OKP","crv":"Ed25519","alg":"EdDSA",
            "x":URL_SAFE_NO_PAD.encode(signing_key(seed)?.public_key().as_ref())}))
    };
    Ok(json!({"keys":[key("stamp-a", 7)?, key("stamp-b", 8)?]}))
}

/// The published set without the keys the fixture has retired.
pub fn published_keys_except(retired: &[String]) -> ConformanceResult<Value> {
    let mut keys = published_keys()?;
    if let Some(list) = keys.get_mut("keys").and_then(Value::as_array_mut) {
        list.retain(|key| {
            key.get("kid")
                .and_then(Value::as_str)
                .is_none_or(|kid| !retired.iter().any(|gone| gone == kid))
        });
    }
    Ok(keys)
}

pub fn claims(organization: &str, now_seconds: i64) -> Value {
    json!({
        "purpose":"fleet-organization-stamp","version":1,"iss":ISSUER,"aud":AUDIENCE,
        "sub":organization,"organization_id":organization,
        "iat":now_seconds - 60,"exp":now_seconds - 60 + LIFETIME_SECONDS
    })
}

pub fn sign(header: &Value, claims: &Value, seed: u8) -> ConformanceResult<String> {
    let signed = format!(
        "{}.{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(header)?),
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims)?)
    );
    let signature = signing_key(seed)?.sign(signed.as_bytes());
    Ok(format!(
        "{signed}.{}",
        URL_SAFE_NO_PAD.encode(signature.as_ref())
    ))
}

pub fn stamp_header() -> Value {
    json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-a"})
}

pub fn stamp(organization: &str, now_seconds: i64) -> ConformanceResult<String> {
    sign(&stamp_header(), &claims(organization, now_seconds), 7)
}

/// A valid statement signed by the published `stamp-b` key (seed 8).
pub fn stamp_under_second_key(organization: &str, now_seconds: i64) -> ConformanceResult<String> {
    stamp_under_second_key_at(organization, now_seconds, 0)
}

/// A valid setup statement under `stamp-b`, issued `issued_offset` seconds
/// after the fixture stamp: different bytes for each offset.
pub fn stamp_under_second_key_at(
    organization: &str,
    now_seconds: i64,
    issued_offset: i64,
) -> ConformanceResult<String> {
    let mut claims = claims(organization, now_seconds);
    set_claim(&mut claims, "iat", json!(now_seconds - 60 + issued_offset))?;
    sign(
        &json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-b"}),
        &claims,
        8,
    )
}

/// Flip one base64url symbol inside the signature, keeping the JWS decodable.
pub fn tamper_signature(statement: &str) -> ConformanceResult<String> {
    let mut bytes = statement.as_bytes().to_vec();
    let index = bytes.len().checked_sub(10).ok_or("statement too short")?;
    let symbol = bytes.get_mut(index).ok_or("statement too short")?;
    *symbol = if *symbol == b'A' { b'B' } else { b'A' };
    Ok(String::from_utf8(bytes)?)
}

pub fn digest(statement: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(statement.as_bytes()))
}

/// Another valid first statement for `organization`, issued `issued_offset`
/// seconds after `stamp`'s: different bytes, no predecessor.
pub fn first_statement(
    organization: &str,
    now_seconds: i64,
    issued_offset: i64,
) -> ConformanceResult<String> {
    let mut claims = claims(organization, now_seconds);
    set_claim(&mut claims, "iat", json!(now_seconds - 60 + issued_offset))?;
    set_claim(
        &mut claims,
        "exp",
        json!(now_seconds - 60 + issued_offset + LIFETIME_SECONDS),
    )?;
    sign(&stamp_header(), &claims, 7)
}

/// A renewal names its predecessor's digest inside its signed claims.
pub fn renewal(
    organization: &str,
    predecessor: &str,
    now_seconds: i64,
    issued_offset: i64,
) -> ConformanceResult<String> {
    let mut claims = claims(organization, now_seconds);
    set_claim(&mut claims, "iat", json!(now_seconds - 60 + issued_offset))?;
    set_claim(
        &mut claims,
        "exp",
        json!(now_seconds - 60 + issued_offset + LIFETIME_SECONDS),
    )?;
    set_claim(&mut claims, "replaces", json!(digest(predecessor)))?;
    sign(&stamp_header(), &claims, 7)
}

pub fn set_claim(claims: &mut Value, name: &str, value: Value) -> ConformanceResult<()> {
    claims
        .as_object_mut()
        .ok_or("claims are not an object")?
        .insert(name.to_owned(), value);
    Ok(())
}

/// The fixture sign-in service: only its published key set, over loopback.
pub struct SignInKeys {
    pub url: String,
    retired: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    fetches: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
impl SignInKeys {
    pub async fn start() -> ConformanceResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/api/auth/jwks", listener.local_addr()?);
        let retired = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let published = retired.clone();
        let fetches = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = fetches.clone();
        let router = axum::Router::new().route(
            "/api/auth/jwks",
            axum::routing::get(move || {
                let published = published.clone();
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                async move {
                    let retired = published
                        .lock()
                        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)?
                        .clone();
                    published_keys_except(&retired)
                        .map(axum::Json)
                        .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
                }
            }),
        );
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
    /// Stop publishing `kid`, as the sign-in service does once every central
    /// holds a statement under its successor and has restarted cold.
    pub fn retire(&self, kid: &str) -> ConformanceResult<()> {
        self.retired
            .lock()
            .map_err(|_| "retired key list poisoned")?
            .push(kid.to_owned());
        Ok(())
    }
    /// How many times central has fetched the published key set.
    pub fn fetches(&self) -> usize {
        self.fetches.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub async fn shutdown(self) -> ConformanceResult<()> {
        let _ = self.stop.send(());
        self.task.await??;
        Ok(())
    }
}

mod statement_refusals;
pub use statement_refusals::{
    StatementRefusalCase, statement_refusal_case, statement_with_header, statement_with_times,
};
