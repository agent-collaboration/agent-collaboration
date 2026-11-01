//! Real native signing and HTTP, against an event-driven RFC7523 receiver.
#![cfg(test)]
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    ClientAssertionLifetime, IssuerKeyId, PersonIntrospectionClient, PersonSessionConfirmer,
    PrivateKeyJwtClient, ServiceClientId,
    identity_contract::ResourceUri,
    organization_authority::{PersonSessionConfirmation, SignInUnavailable},
};
use collaboration_central_tokio::{DeliveryConfig, TokioCentralPlatform, TokioEd25519ClientSigner};
use ring::signature::{ED25519, UnparsedPublicKey};
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};

#[path = "native_http/support.rs"]
mod configuration;

const PERSON_TOKEN: &str = "public.fixture-person-proof";
const NOW_SECONDS: i64 = 2_208_988_800;
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(NOW_SECONDS * 1_000)
    }
}
#[derive(Default)]
struct ScriptedIds(AtomicU64);
impl IdSource for ScriptedIds {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::from_u128(
            0x019f0000000070008000000000000000 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertionHeader {
    alg: String,
    typ: String,
    kid: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AssertionClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    jti: String,
}
struct Receiver {
    base: String,
    public_key: Vec<u8>,
    confirmation: Vec<u8>,
    seen: Mutex<Vec<(String, String)>>,
    errors: Mutex<Vec<String>>,
}
fn validate_request(
    state: &Receiver,
    uri: &Uri,
    headers: &HeaderMap,
    body: &str,
) -> Result<String, configuration::TestError> {
    if headers.contains_key("authorization")
        || headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            != Some("application/x-www-form-urlencoded")
    {
        return Err("wrong introspection authentication channel/content type".into());
    }
    let form: BTreeMap<_, _> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    if form.len() != 4
        || form.get("token").map(String::as_str) != Some(PERSON_TOKEN)
        || form.get("client_id").map(String::as_str) != Some("pinned-central")
        || form.get("client_assertion_type").map(String::as_str)
            != Some("urn:ietf:params:oauth:client-assertion-type:jwt-bearer")
    {
        return Err("wrong RFC7523/RFC7662 form".into());
    }
    let assertion = form
        .get("client_assertion")
        .ok_or("missing client assertion")?;
    let (signed, signature) = assertion.rsplit_once('.').ok_or("malformed assertion")?;
    let (header, claims) = signed.split_once('.').ok_or("malformed signing input")?;
    let header: AssertionHeader = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header)?)?;
    if header.alg != "EdDSA" || header.typ != "JWT" || header.kid != "rfc8032-test-1" {
        return Err("wrong assertion protected header".into());
    }
    UnparsedPublicKey::new(&ED25519, &state.public_key)
        .verify(signed.as_bytes(), &URL_SAFE_NO_PAD.decode(signature)?)
        .map_err(|_| "bad assertion signature")?;
    let claims: AssertionClaims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims)?)?;
    if claims.iss != "pinned-central"
        || claims.sub != "pinned-central"
        || claims.aud != format!("{}{uri}", state.base)
        || claims.iat != NOW_SECONDS
        || claims.exp != NOW_SECONDS + 60
        || durable_steps::Uuid::parse_str(&claims.jti)?.get_version_num() != 7
    {
        return Err("wrong assertion claims".into());
    }
    Ok(claims.jti)
}
async fn receive(
    State(state): State<Arc<Receiver>>,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    match validate_request(&state, &uri, &headers, &body) {
        Ok(token_id) => {
            if let Ok(mut seen) = state.seen.lock() {
                seen.push((uri.to_string(), token_id));
            }
        }
        Err(error) => {
            if let Ok(mut errors) = state.errors.lock() {
                errors.push(error.to_string());
            }
            return Response::builder()
                .status(StatusCode::BAD_REQUEST)
                .body(Body::empty())
                .unwrap_or_default();
        }
    }
    let (status, body) = match uri.path() {
        "/introspect" => (200, state.confirmation.clone()),
        "/malformed" => (200, b"{}".to_vec()),
        "/redirect" => (307, Vec::new()),
        _ => (401, Vec::new()),
    };
    Response::builder()
        .status(status)
        .header("location", "/unexpected")
        .body(Body::from(body))
        .unwrap_or_default()
}
fn hex(value: &str) -> Result<Vec<u8>, configuration::TestError> {
    (0..value.len())
        .step_by(2)
        .map(|offset| {
            Ok(u8::from_str_radix(
                value.get(offset..offset + 2).ok_or("invalid fixture hex")?,
                16,
            )?)
        })
        .collect()
}

// The private seed below is RFC8032's published test1 seed. No real credential,
// provider clock or session store is involved. The receiver checks actual
// signatures, exact endpoint audience, single-use IDs and the form channel.
#[tokio::test]
async fn private_key_jwt_signs_real_http_introspection_and_refuses_redirects()
-> Result<(), configuration::TestError> {
    let secret =
        include_bytes!("../../collaboration-central-core/tests/fixtures/rfc8032-test1.pkcs8");
    let signer = TokioEd25519ClientSigner::from_pkcs8(
        IssuerKeyId::try_from("rfc8032-test-1".to_owned())?,
        secret,
    )?;
    if signer.public_key()
        != hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")?
    {
        return Err("client signer differs from RFC8032 public test key1".into());
    }
    let producer: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../collaboration-central-core/tests/fixtures/person-identity/K5.fixtures.json"
    ))?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let receiver = Arc::new(Receiver {
        base: format!("http://{}", listener.local_addr()?),
        public_key: signer.public_key().to_vec(),
        confirmation: serde_json::to_vec(&producer["examples"]["active"])?,
        seen: Mutex::default(),
        errors: Mutex::default(),
    });
    let app = Router::new().fallback(receive).with_state(receiver.clone());
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let directory = tempfile::tempdir()?;
    let platform = TokioCentralPlatform::new(
        "https://unused.example.invalid/jwks",
        Arc::new(FixedClock),
        |_| {},
        directory.path(),
        configuration::registry_config()?,
        DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
        Duration::from_secs(2),
        Duration::from_secs(2),
    )
    .await?;
    let ids = Arc::new(ScriptedIds::default());
    let proof = async {
        for path in [
            "/introspect",
            "/introspect",
            "/redirect",
            "/malformed",
            "/refused",
        ] {
            let client = PrivateKeyJwtClient::new(
                ServiceClientId::try_from("pinned-central".to_owned())?,
                signer.clone(),
                Arc::new(FixedClock),
                ids.clone(),
                ClientAssertionLifetime::from_seconds(60)?,
            )?;
            let endpoint = ResourceUri::try_from(format!("{}{path}", receiver.base))?;
            let confirmer =
                PersonIntrospectionClient::new(client, platform.service_form_transport(), endpoint);
            let answer = confirmer.confirm_person_session(PERSON_TOKEN).await;
            if path == "/introspect" {
                if !matches!(answer, Ok(PersonSessionConfirmation::Active { .. })) {
                    return Err("valid authenticated introspection refused".into());
                }
            } else if answer != Err(SignInUnavailable::ConfirmationUnreachable) {
                return Err("bad introspection reply was accepted".into());
            }
        }
        let errors = receiver
            .errors
            .lock()
            .map_err(|_| "receiver errors poisoned")?;
        if !errors.is_empty() {
            return Err(format!("assertion receiver errors: {errors:?}").into());
        }
        let seen = receiver
            .seen
            .lock()
            .map_err(|_| "receiver trace poisoned")?;
        let ids: std::collections::BTreeSet<_> = seen.iter().map(|(_, id)| id).collect();
        if seen.len() != 5 || ids.len() != 5 || seen.iter().any(|(path, _)| path == "/unexpected") {
            return Err("assertions were replayed, lost or redirected".into());
        }
        println!("CLIENT_AUTH_HTTP accepted=2 refused=3 requests=5 uniqueJti=5 unexpected=0");
        Ok::<_, configuration::TestError>(())
    }
    .await;
    let _ = shutdown.send(());
    let server_result = server.await?;
    let stopped = platform.shutdown().await;
    server_result?;
    stopped?;
    proof
}
