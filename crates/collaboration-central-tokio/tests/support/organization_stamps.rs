//! Real Ed25519 stamps, the sign-in key set they verify against, and an HTTP
//! client for the private link's Unix socket.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_conformance::ConformanceResult;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(crate) const ISSUER: &str = "https://sign-in.example.invalid";
pub(crate) const AUDIENCE: &str = "https://central.example.invalid/identity";
/// The fixed business time every link test's Fleet objects run at.
pub(crate) const NOW_SECONDS: i64 = 1_900_000_000;
/// 120 days, the owner's statement lifetime.
pub(crate) const LIFETIME_SECONDS: i64 = 10_368_000;

pub(crate) fn signing_key(seed: u8) -> ConformanceResult<Ed25519KeyPair> {
    // Public test fixture only: never a deployment signing key.
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).map_err(|_| "fixture seed".into())
}

/// The keys the fixture sign-in service publishes: `stamp-a` (seed 7) and
/// `stamp-b` (seed 8). Seed 9 signs with an unpublished key.
pub(crate) fn published_keys() -> ConformanceResult<Value> {
    let key = |kid: &str, seed: u8| -> ConformanceResult<Value> {
        Ok(json!({"kid":kid,"kty":"OKP","crv":"Ed25519","alg":"EdDSA",
            "x":URL_SAFE_NO_PAD.encode(signing_key(seed)?.public_key().as_ref())}))
    };
    Ok(json!({"keys":[key("stamp-a", 7)?, key("stamp-b", 8)?]}))
}

pub(crate) fn claims(organization: &str) -> Value {
    json!({
        "purpose":"fleet-organization-stamp","version":1,"iss":ISSUER,"aud":AUDIENCE,
        "sub":organization,"organization_id":organization,
        "iat":NOW_SECONDS - 60,"exp":NOW_SECONDS - 60 + LIFETIME_SECONDS
    })
}

pub(crate) fn sign(header: &Value, claims: &Value, seed: u8) -> ConformanceResult<String> {
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

pub(crate) fn stamp_header() -> Value {
    json!({"typ":"fleet-organization-stamp+jwt","alg":"EdDSA","kid":"stamp-a"})
}

pub(crate) fn stamp(organization: &str) -> ConformanceResult<String> {
    sign(&stamp_header(), &claims(organization), 7)
}

/// Flip one base64url symbol inside the signature, keeping the JWS decodable.
pub(crate) fn tamper_signature(statement: &str) -> ConformanceResult<String> {
    let mut bytes = statement.as_bytes().to_vec();
    let index = bytes.len().checked_sub(10).ok_or("statement too short")?;
    let symbol = bytes.get_mut(index).ok_or("statement too short")?;
    *symbol = if *symbol == b'A' { b'B' } else { b'A' };
    Ok(String::from_utf8(bytes)?)
}

pub(crate) fn digest(statement: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(statement.as_bytes()))
}

/// A renewal names its predecessor's digest inside its signed claims.
pub(crate) fn renewal(
    organization: &str,
    predecessor: &str,
    issued_offset: i64,
) -> ConformanceResult<String> {
    let mut claims = claims(organization);
    set_claim(&mut claims, "iat", json!(NOW_SECONDS - 60 + issued_offset))?;
    set_claim(
        &mut claims,
        "exp",
        json!(NOW_SECONDS - 60 + issued_offset + LIFETIME_SECONDS),
    )?;
    set_claim(&mut claims, "replaces", json!(digest(predecessor)))?;
    sign(&stamp_header(), &claims, 7)
}

pub(crate) fn set_claim(claims: &mut Value, name: &str, value: Value) -> ConformanceResult<()> {
    claims
        .as_object_mut()
        .ok_or("claims are not an object")?
        .insert(name.to_owned(), value);
    Ok(())
}

/// The fixture sign-in service: only its published key set, over loopback.
pub(crate) struct SignInKeys {
    pub url: String,
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
impl SignInKeys {
    pub(crate) async fn start() -> ConformanceResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}/api/auth/jwks", listener.local_addr()?);
        let router = axum::Router::new().route(
            "/api/auth/jwks",
            axum::routing::get(|| async {
                published_keys()
                    .map(axum::Json)
                    .map_err(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR)
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
        Ok(Self { url, stop, task })
    }
    pub(crate) async fn shutdown(self) -> ConformanceResult<()> {
        let _ = self.stop.send(());
        self.task.await??;
        Ok(())
    }
}

/// One HTTP/1.1 exchange over the link socket; the server closes after it.
pub(crate) async fn link_post(
    socket: &Path,
    path: &str,
    body: &Value,
) -> ConformanceResult<(u16, Vec<u8>)> {
    let mut stream = tokio::net::UnixStream::connect(socket).await?;
    let body = serde_json::to_vec(body)?;
    let head = format!(
        "POST {path} HTTP/1.1\r\nhost: central.invalid\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(&body).await?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await?;
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("link response has no header terminator")?;
    let (head, rest) = response.split_at(split);
    let head = std::str::from_utf8(head)?;
    let status = head
        .split_whitespace()
        .nth(1)
        .ok_or("link response has no status")?
        .parse()?;
    let mut payload = rest.get(4..).ok_or("response ends at headers")?.to_vec();
    if head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        payload = dechunk(&payload)?;
    }
    Ok((status, payload))
}

fn dechunk(mut bytes: &[u8]) -> ConformanceResult<Vec<u8>> {
    let mut body = Vec::new();
    loop {
        let line = bytes
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or("chunk size line")?;
        let (size_line, rest) = bytes.split_at(line);
        let size = usize::from_str_radix(std::str::from_utf8(size_line)?.trim(), 16)?;
        bytes = rest.get(2..).ok_or("chunk size line terminator")?;
        if size == 0 {
            return Ok(body);
        }
        body.extend_from_slice(bytes.get(..size).ok_or("short chunk")?);
        bytes = bytes.get(size + 2..).ok_or("chunk terminator")?;
    }
}
