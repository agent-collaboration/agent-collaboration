//! Real directory signing/form/bearer HTTP, shared with the Worker observation.
#![cfg(all(test, feature = "conformance"))]
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::Response,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_central_core::{
    DirectoryClientProbe, IssuerKeyId, ServiceClientId,
    identity_contract::{CentralRecord, ResourceUri},
    observe_directory_client,
};
use collaboration_central_tokio::{DeliveryConfig, TokioCentralPlatform, TokioEd25519ClientSigner};
use ring::signature::{ED25519, UnparsedPublicKey};
use runtime_primitives::{Clock, SyncTime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};

#[path = "native_http/support.rs"]
mod configuration;
struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(2_208_988_800_000)
    }
}
#[derive(Deserialize)]
struct Corpus {
    input: DirectoryClientProbe,
    steps: Vec<Step>,
    expected: Value,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Step {
    name: String,
    operation: String,
    scope: String,
    request: Value,
    grant: GrantReply,
    operation_reply: HttpReply,
    expect_operation: bool,
    expected: Value,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum GrantReply {
    Credential {
        scope: String,
    },
    Response {
        status: u16,
        content_type: String,
        body: String,
    },
    TransportFailure,
}
#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum HttpReply {
    Response {
        status: u16,
        content_type: String,
        body: String,
    },
    TransportFailure,
}
#[derive(Serialize)]
#[serde(tag = "channel", rename_all = "camelCase")]
enum ObservedRequest {
    Token {
        step: usize,
        name: String,
        uri: String,
        assertion: String,
        jti: String,
        method: String,
        headers: BTreeMap<String, String>,
        #[serde(rename = "formBody")]
        form_body: String,
    },
    Operation {
        step: usize,
        name: String,
        uri: String,
        body: String,
        token: String,
        method: String,
        headers: BTreeMap<String, String>,
    },
}
struct Receiver {
    base: String,
    public_key: Vec<u8>,
    steps: Vec<Step>,
    next: usize,
    pending: Option<String>,
    requests: Vec<ObservedRequest>,
    errors: Vec<String>,
}

// An independent RFC7523 wire oracle: each required assertion claim has its
// real JSON type, rather than null-fallback indexing into a partial envelope.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClientAssertionClaims {
    iss: String,
    sub: String,
    aud: String,
    iat: i64,
    exp: i64,
    jti: String,
}

fn validate_assertion(
    assertion: &str,
    audience: &str,
    public_key: &[u8],
) -> Result<String, configuration::TestError> {
    let (signed, signature) = assertion.rsplit_once('.').ok_or("malformed assertion")?;
    let (header, claims) = signed.split_once('.').ok_or("malformed signing input")?;
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(header)?)?;
    let claims: ClientAssertionClaims = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims)?)?;
    if header != json!({"alg":"EdDSA","typ":"JWT","kid":"rfc8032-test-1"})
        || claims.iss != "pinned-directory"
        || claims.sub != "pinned-directory"
        || claims.aud != audience
        || claims.iat != 2_208_988_800_i64
        || claims.exp != 2_208_988_860_i64
    {
        return Err("incorrect directory client assertion".into());
    }
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(signed.as_bytes(), &URL_SAFE_NO_PAD.decode(signature)?)
        .map_err(|_| "invalid directory assertion signature")?;
    if durable_steps::Uuid::parse_str(&claims.jti)?.get_version_num() != 7 {
        return Err("assertion jti is not UUIDv7".into());
    }
    Ok(claims.jti)
}
fn response(reply: &HttpReply) -> Response {
    match reply {
        HttpReply::Response {
            status,
            content_type,
            body,
        } => Response::builder()
            .status(*status)
            .header("content-type", content_type)
            .header("location", "/unexpected")
            .body(Body::from(body.clone()))
            .unwrap_or_default(),
        HttpReply::TransportFailure => Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(Body::from_stream(futures_util::stream::once(async {
                Err::<Vec<u8>, _>(std::io::Error::other("fixture response transport failure"))
            })))
            .unwrap_or_default(),
    }
}
fn reject(state: &mut Receiver, message: impl Into<String>) -> Response {
    state.errors.push(message.into());
    Response::builder()
        .status(StatusCode::BAD_REQUEST)
        .body(Body::empty())
        .unwrap_or_default()
}
fn token_request(
    state: &mut Receiver,
    method: &Method,
    uri: &Uri,
    headers: &BTreeMap<String, String>,
    body: &str,
) -> Response {
    if state.pending.is_some() {
        return reject(state, "new credential requested before pending operation");
    }
    let index = state.next;
    let Some(step) = state.steps.get(index) else {
        return reject(state, "unexpected token request");
    };
    let pairs: Vec<_> = url::form_urlencoded::parse(body.as_bytes())
        .into_owned()
        .collect();
    let pair_count = pairs.len();
    let form: BTreeMap<_, _> = pairs.into_iter().collect();
    let destination = format!("{}/{}", state.base, step.operation);
    if pair_count != 6
        || headers.contains_key("authorization")
        || form.len() != 6
        || form.get("grant_type").map(String::as_str) != Some("client_credentials")
        || form.get("client_id").map(String::as_str) != Some("pinned-directory")
        || form.get("client_assertion_type").map(String::as_str)
            != Some("urn:ietf:params:oauth:client-assertion-type:jwt-bearer")
        || form.get("resource") != Some(&destination)
        || form.get("scope") != Some(&step.scope)
    {
        return reject(
            state,
            "wrong directory credential form/resource/scope/channel",
        );
    }
    let Some(assertion) = form.get("client_assertion") else {
        return reject(state, "no directory client assertion");
    };
    let endpoint = format!("{}{uri}", state.base);
    let jti = match validate_assertion(assertion, &endpoint, &state.public_key) {
        Ok(jti) => jti,
        Err(error) => return reject(state, error.to_string()),
    };
    state.requests.push(ObservedRequest::Token {
        step: index,
        name: step.name.clone(),
        uri: endpoint,
        assertion: assertion.clone(),
        jti,
        method: method.as_str().into(),
        headers: headers.clone(),
        form_body: body.into(),
    });
    let token = format!("public.fixture-service-token.{index}");
    let reply = match &step.grant {
        GrantReply::Credential { scope } => response(&HttpReply::Response {
            status: 200,
            content_type: "application/json".into(),
            body:
                json!({"access_token":token,"token_type":"Bearer","scope":scope,"expires_in":300})
                    .to_string(),
        }),
        GrantReply::Response {
            status,
            content_type,
            body,
        } => response(&HttpReply::Response {
            status: *status,
            content_type: content_type.clone(),
            body: body.clone(),
        }),
        GrantReply::TransportFailure => response(&HttpReply::TransportFailure),
    };
    if step.expect_operation {
        state.pending = Some(token);
    } else {
        state.next += 1;
    }
    reply
}
fn operation_request(
    state: &mut Receiver,
    method: &Method,
    uri: &Uri,
    headers: &BTreeMap<String, String>,
    body: &str,
) -> Response {
    let index = state.next;
    let Some(token) = state.pending.as_ref() else {
        return reject(state, "central call without a successful credential");
    };
    let Some(step) = state.steps.get(index) else {
        return reject(state, "unexpected central call");
    };
    if uri.to_string() != format!("/{}", step.operation)
        || headers.get("authorization").map(String::as_str)
            != Some(format!("Bearer {token}").as_str())
        || serde_json::from_str::<Value>(body).ok() != Some(step.request.clone())
        || body.contains(token)
        || headers.contains_key("client_assertion")
    {
        return reject(state, "wrong central body/address/bearer channel");
    }
    state.requests.push(ObservedRequest::Operation {
        step: index,
        name: step.name.clone(),
        uri: format!("{}{uri}", state.base),
        body: body.into(),
        token: token.clone(),
        method: method.as_str().into(),
        headers: headers.clone(),
    });
    let reply = response(&step.operation_reply);
    state.pending = None;
    state.next += 1;
    reply
}
async fn receive(
    State(state): State<Arc<Mutex<Receiver>>>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: String,
) -> Response {
    let Ok(mut state) = state.lock() else {
        return Response::default();
    };
    let collected = headers
        .iter()
        .map(|(name, value)| {
            value
                .to_str()
                .map(|value| (name.as_str().to_owned(), value.to_owned()))
        })
        .collect::<Result<BTreeMap<_, _>, _>>();
    let Ok(headers) = collected else {
        return reject(&mut state, "non-text request header");
    };
    if method != Method::POST {
        return reject(&mut state, "service request wasn't POST");
    }
    let media = headers.get("content-type").map(String::as_str);
    match (uri.path(), media) {
        ("/oauth/token", Some("application/x-www-form-urlencoded")) => {
            token_request(&mut state, &method, &uri, &headers, &body)
        }
        (_, Some("application/json")) => {
            operation_request(&mut state, &method, &uri, &headers, &body)
        }
        _ => reject(&mut state, "unexpected service path or content type"),
    }
}
fn central(base: &str) -> Result<CentralRecord, configuration::TestError> {
    Ok(serde_json::from_value(json!({"addresses":{
        "sessionEndpoint":"https://central.example.invalid/v1/session","machineAddress":"https://central.example.invalid/machine",
        "adminAddress":"https://central.example.invalid/admin","admissionAddress":"https://central.example.invalid/admission",
        "standingAddress":"https://central.example.invalid/standing","machineTokenAddress":"https://central.example.invalid/machine-token",
        "receiver":"019f0000-0000-7000-8000-000000000002","relay":"019f0000-0000-7000-8000-000000000003"},
        "directoryCalls":{"approvalAuthorizeAddress":format!("{base}/authorize"),"approvalActivateAddress":format!("{base}/activate"),
            "approvalCancelAddress":format!("{base}/cancel"),"initializationAddress":format!("{base}/initialize")}}))?)
}

// Actual native signing and HTTP remain real. The scripted issuer and central
// are stand-ins; no K7 mint, durable authority or SO7 is claimed by this run.
#[tokio::test]
async fn directory_service_calls_match_the_shared_real_http_corpus()
-> Result<(), configuration::TestError> {
    let corpus: Corpus =
        serde_json::from_str(include_str!("fixtures/issuer_directory_cases.json"))?;
    let signer = TokioEd25519ClientSigner::from_pkcs8(
        IssuerKeyId::try_from("rfc8032-test-1".to_owned())?,
        include_bytes!("../../collaboration-central-core/tests/fixtures/rfc8032-test1.pkcs8"),
    )?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", listener.local_addr()?);
    let central = central(&base)?;
    let receiver = Arc::new(Mutex::new(Receiver {
        base: base.clone(),
        public_key: signer.public_key().to_vec(),
        steps: corpus.steps,
        next: 0,
        pending: None,
        requests: Vec::new(),
        errors: Vec::new(),
    }));
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
    let proof=async {
        let observations=observe_directory_client(signer,platform.service_http_transport(),ServiceClientId::try_from("pinned-directory".to_owned())?,
            ResourceUri::try_from(format!("{base}/oauth/token"))?,&central,corpus.input).await?;
        let observed=serde_json::to_value(observations)?;
        if observed!=corpus.expected {return Err(format!("directory outcomes differ: {observed}").into());}
        let receiver=receiver.lock().map_err(|_|"receiver poisoned")?;
        if !receiver.errors.is_empty() || receiver.next!=receiver.steps.len() || receiver.pending.is_some() {
            return Err(format!("directory HTTP errors: {:?}; completed {}/{}",receiver.errors,receiver.next,receiver.steps.len()).into());
        }
        let mut token_ids=BTreeSet::new();let mut token_count=0;let mut operation_count=0;
        let mut authorize_body:Option<&str>=None;
        for request in &receiver.requests {
            match request {
                ObservedRequest::Token {jti,..}=>{token_count+=1;if !token_ids.insert(jti) {return Err("directory assertion was replayed".into());}},
                ObservedRequest::Operation {step,body,..}=>{
                    operation_count+=1;
                    if receiver.steps[*step].operation=="authorize" {
                        if authorize_body.is_some_and(|previous|previous!=body) {return Err("resend changed authorize body".into());}
                        authorize_body=Some(body);
                    }
                },
            }
            println!("DIRECTORY_HTTP_REQUEST {}",serde_json::to_string(request)?);
        }
        let expected_operations=receiver.steps.iter().filter(|step|step.expect_operation).count();
        if token_count!=receiver.steps.len() || operation_count!=expected_operations {return Err("wrong directory request counts".into());}
        // Per-step expected values also stay in the fixture for the independent
        // Worker driver, rather than using the native verdict as its oracle.
        for (step,expected) in receiver.steps.iter().zip(corpus.expected.as_array().ok_or("fixture expected array")?) {
            if &step.expected!=expected {return Err("fixture per-step oracle mismatch".into());}
        }
        println!("DIRECTORY_HTTP_RESULT {observed}");
        println!("DIRECTORY_HTTP_COUNTS token={token_count} operation={operation_count} uniqueJti={} unexpected=0",token_ids.len());
        println!("DIRECTORY_HTTP_CENTRAL {}",serde_json::to_string(&central)?);
        Ok::<_,configuration::TestError>(())
    }.await;
    let _ = shutdown.send(());
    let server_result = server.await?;
    let stopped = platform.shutdown().await;
    server_result?;
    stopped?;
    proof
}
