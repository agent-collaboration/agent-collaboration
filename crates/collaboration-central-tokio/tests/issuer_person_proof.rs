//! Same signed corpus and observation surface as the real Worker probe.
#![cfg(all(test, feature = "conformance"))]
use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use collaboration_central_core::{
    PersonProofObservation, PersonProofProbe, identity_contract::ResourceUri, observe_person_proof,
    organization_authority::PersonTokenPolicy,
};
use collaboration_central_tokio::{DeliveryConfig, TokioCentralPlatform};
use runtime_primitives::{Clock, SyncTime};
use serde::Deserialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};

#[path = "native_http/support.rs"]
mod configuration;
#[path = "support/paused_http_clock.rs"]
mod paused_http_clock;

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(2_208_988_800_000)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Corpus {
    policy: PolicyFixture,
    cases: Vec<PersonCase>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PolicyFixture {
    issuer: String,
    admin_audience: String,
    operator_audience: String,
    operator_client: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PersonCase {
    name: String,
    input: PersonProofProbe,
    key_responses: Vec<KeyResponse>,
    expected_fetches: usize,
    expected: PersonProofObservation,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct KeyResponse {
    body: String,
    cache_control: String,
    status: u16,
}

#[derive(Default)]
struct KeyServer {
    responses: VecDeque<KeyResponse>,
    requests: Vec<(String, Option<String>)>,
}

async fn serve_keys(
    State(state): State<Arc<Mutex<KeyServer>>>,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    let Ok(mut state) = state.lock() else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap_or_default();
    };
    state.requests.push((
        uri.to_string(),
        headers
            .get("cache-control")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    ));
    let Some(answer) = state.responses.pop_front() else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap_or_default();
    };
    Response::builder()
        .status(answer.status)
        .header("cache-control", answer.cache_control)
        // Following even this local redirect is a failure: record its path.
        .header("location", "/untrusted")
        .body(Body::from(answer.body))
        .unwrap_or_default()
}

// Catches token-controlled sources, stale/extra fetches, ignored critical
// headers and algorithm confusion. Oracle: the producer K5/K6 positive control,
// RFC8032 signatures and the named PD boundaries in the shared JSON corpus.
// Actual HTTP, host crypto, common parser and policy remain real. Introspection
// alone is a named stand-in, so this does not claim authenticated SO7.
#[tokio::test(start_paused = true)]
async fn native_person_proofs_match_the_shared_host_corpus() -> Result<(), configuration::TestError>
{
    let corpus: Corpus = serde_json::from_str(include_str!("fixtures/issuer_person_cases.json"))?;
    let policy = PersonTokenPolicy::new(
        corpus.policy.issuer,
        corpus.policy.admin_audience,
        corpus.policy.operator_audience,
        corpus.policy.operator_client,
    )?;
    let state = Arc::new(Mutex::new(KeyServer::default()));
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address =
        ResourceUri::try_from(format!("http://{}/configured/jwks", listener.local_addr()?))?;
    let (shutdown, stopped) = oneshot::channel();
    let app = Router::new().fallback(serve_keys).with_state(state.clone());
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
    let proof = async {
        for case in corpus.cases {
            {
                let mut state = state.lock().map_err(|_| "fixture state poisoned")?;
                state.requests.clear();
                state.responses = case.key_responses.into();
            }
            let clock = paused_http_clock::PausedHttpClock::before_case();
            let observation = observe_person_proof(
                platform.clone(),
                platform.issuer_key_source(address.clone()),
                &policy,
                case.input,
            )
            .await;
            clock.require_no_advance(&case.name)?;
            let state = state.lock().map_err(|_| "fixture state poisoned")?;
            if observation != case.expected {
                return Err(format!(
                    "{}: expected {:?}, observed {observation:?}",
                    case.name, case.expected
                )
                .into());
            }
            if state.requests.len() != case.expected_fetches || !state.responses.is_empty() {
                return Err(format!(
                    "{}: expected {} key fetches; observed {} ({} responses unused)",
                    case.name,
                    case.expected_fetches,
                    state.requests.len(),
                    state.responses.len()
                )
                .into());
            }
            if state.requests.iter().any(|(path, header)| {
                path != "/configured/jwks" || header.as_deref() != Some("no-cache, no-store")
            }) {
                return Err(format!(
                    "{}: wrong outbound path/cache policy: {:?}",
                    case.name, state.requests
                )
                .into());
            }
            println!(
                "ISSUER_PERSON_CASE {} fetches={} {}",
                case.name,
                state.requests.len(),
                serde_json::to_string(&observation)?
            );
        }
        Ok::<_, configuration::TestError>(())
    }
    .await;
    let _ = shutdown.send(());
    let server_result = server.await?;
    let platform_result = platform.shutdown().await;
    server_result?;
    platform_result?;
    proof
}
