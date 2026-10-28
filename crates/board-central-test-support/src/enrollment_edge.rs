//! Fixture Access signs provenance while preserving the producer's fresh Bearer bytes.
use crate::{BOUND, FixtureResult};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use collaboration_central_core::{AccessCaller, AccessClientId, EnrollMachine, EnrollmentDecision};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message as UpstreamMessage, RequestBuilderExt};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::task::JoinSet;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub(crate) const CLIENT_ID: &str = "producer-fixture.access";
pub(crate) const CLIENT_SECRET: &str = "producer-access-secret-canary";
pub(crate) const OWNER_TOKEN: &str = "producer-owner-token-canary";

enum FixtureReply {
    Decision(EnrollmentDecision),
    Raw { status: StatusCode, body: Vec<u8> },
    Redirect(String),
}

#[derive(Clone)]
struct EdgeState {
    upstream: String,
    client: reqwest::Client,
    machine_assertion: String,
    owner_assertion: String,
    observations: Arc<Mutex<Vec<EnrollMachine>>>,
    forced_reply: Arc<Mutex<Option<FixtureReply>>>,
    authorization_hashes: Arc<Mutex<Vec<String>>>,
    admin_calls: Arc<AtomicUsize>,
    sessions: Arc<AtomicUsize>,
    rejections: Arc<Mutex<Vec<u16>>>,
    rejected: Arc<tokio::sync::Notify>,
    stop: CancellationToken,
    tasks: TaskTracker,
}

pub(crate) struct EnrollmentAccessEdge {
    pub endpoint: String,
    state: EdgeState,
    listener: JoinSet<Result<(), std::io::Error>>,
}
impl EnrollmentAccessEdge {
    pub async fn start(upstream: &str) -> FixtureResult<Self> {
        use collaboration_central_conformance::{FIXTURE_KID, fixture_claims, sign_assertion};
        let state = EdgeState {
            upstream: upstream.to_owned(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(BOUND)
                .build()?,
            machine_assertion: sign_assertion(
                &fixture_claims(
                    AccessCaller::Machine {
                        client_id: AccessClientId::new(CLIENT_ID)?,
                    },
                    false,
                ),
                FIXTURE_KID,
            )?,
            owner_assertion: collaboration_central_conformance::owner_assertion()?,
            observations: Arc::default(),
            forced_reply: Arc::default(),
            authorization_hashes: Arc::default(),
            admin_calls: Arc::default(),
            sessions: Arc::default(),
            rejections: Arc::default(),
            rejected: Arc::default(),
            stop: CancellationToken::new(),
            tasks: TaskTracker::new(),
        };
        let app = axum::Router::new()
            .route("/v1/session", get(upgrade))
            .route("/admin/v1/machines/enroll", post(enroll))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/v1/session", listener.local_addr()?);
        let stop = state.stop.clone();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(stop.cancelled_owned())
                .await
        });
        Ok(Self {
            endpoint,
            state,
            listener: tasks,
        })
    }
    pub fn admin_calls(&self) -> usize {
        self.state.admin_calls.load(Ordering::SeqCst)
    }
    pub fn sessions(&self) -> usize {
        self.state.sessions.load(Ordering::SeqCst)
    }
    pub fn observations(&self) -> FixtureResult<Vec<EnrollMachine>> {
        Ok(self
            .state
            .observations
            .lock()
            .map_err(|_| "enrollment observation lock poisoned")?
            .clone())
    }
    pub fn authorization_hashes(&self) -> FixtureResult<Vec<String>> {
        Ok(self
            .state
            .authorization_hashes
            .lock()
            .map_err(|_| "authorization observation lock poisoned")?
            .clone())
    }
    pub fn force_reply(&self, reply: EnrollmentDecision) -> FixtureResult {
        *self
            .state
            .forced_reply
            .lock()
            .map_err(|_| "enrollment reply lock poisoned")? = Some(FixtureReply::Decision(reply));
        Ok(())
    }
    pub fn raw_reply(&self, status: u16, body: Vec<u8>) -> FixtureResult {
        *self
            .state
            .forced_reply
            .lock()
            .map_err(|_| "enrollment reply lock poisoned")? = Some(FixtureReply::Raw {
            status: StatusCode::from_u16(status)?,
            body,
        });
        Ok(())
    }
    pub fn redirect_reply(&self, location: String) -> FixtureResult {
        *self
            .state
            .forced_reply
            .lock()
            .map_err(|_| "enrollment reply lock poisoned")? =
            Some(FixtureReply::Redirect(location));
        Ok(())
    }
    pub async fn wait_for_credential_rejection(&self) -> FixtureResult<u16> {
        tokio::time::timeout(BOUND, async {
            loop {
                let notified = self.state.rejected.notified();
                if let Some(status) = self
                    .state
                    .rejections
                    .lock()
                    .map_err(|_| "session rejection lock poisoned")?
                    .first()
                    .copied()
                {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(status);
                }
                notified.await;
            }
        })
        .await?
    }
    pub async fn shutdown(mut self) -> FixtureResult {
        self.state.stop.cancel();
        self.state.tasks.close();
        while let Some(result) = self.listener.join_next().await {
            result??;
        }
        tokio::time::timeout(BOUND, self.state.tasks.wait()).await?;
        Ok(())
    }
}
impl Drop for EnrollmentAccessEdge {
    fn drop(&mut self) {
        self.state.stop.cancel();
        self.state.tasks.close();
        self.listener.abort_all();
    }
}

fn header_is(headers: &HeaderMap, name: &str, expected: &str) -> bool {
    headers.get(name).and_then(|value| value.to_str().ok()) == Some(expected)
}

async fn enroll(
    State(state): State<EdgeState>,
    headers: HeaderMap,
    axum::Json(body): axum::Json<serde_json::Value>,
) -> Response {
    state.admin_calls.fetch_add(1, Ordering::SeqCst);
    if !header_is(&headers, "cf-access-token", OWNER_TOKEN) {
        return StatusCode::FORBIDDEN.into_response();
    }
    let request = body
        .get("value")
        .cloned()
        .and_then(|value| serde_json::from_value::<EnrollMachine>(value).ok());
    let Some(request) = request else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    {
        let Ok(mut observations) = state.observations.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        observations.push(request);
    }
    let forced = state
        .forced_reply
        .lock()
        .ok()
        .and_then(|mut reply| reply.take());
    if let Some(reply) = forced {
        return match reply {
            FixtureReply::Decision(decision) => axum::Json(decision).into_response(),
            FixtureReply::Raw { status, body } => (status, body).into_response(),
            FixtureReply::Redirect(location) => {
                (StatusCode::TEMPORARY_REDIRECT, [("location", location)]).into_response()
            }
        };
    }
    let response = state
        .client
        .post(format!("{}/admin/v1/machines/enroll", state.upstream))
        .header("Cf-Access-Jwt-Assertion", &state.owner_assertion)
        .json(&body)
        .send()
        .await;
    match response {
        Ok(response) => {
            let status = response.status();
            match response.bytes().await {
                Ok(bytes) => {
                    (status, [("content-type", "application/json")], bytes).into_response()
                }
                Err(_) => StatusCode::BAD_GATEWAY.into_response(),
            }
        }
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}

async fn upgrade(
    State(state): State<EdgeState>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    if !header_is(&headers, "cf-access-client-id", CLIENT_ID)
        || !header_is(&headers, "cf-access-client-secret", CLIENT_SECRET)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(authorization) = headers.get("authorization").cloned() else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    if let Ok(mut observed) = state.authorization_hashes.lock() {
        observed.push(format!("{:x}", Sha256::digest(authorization.as_bytes())));
    } else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let token = state.tasks.token();
    upgrade
        .on_upgrade(move |socket| async move {
            let _token = token;
            // A wrong machine credential is rejected by central, never repaired by this edge.
            let _outcome = bridge(socket, state, authorization).await;
        })
        .into_response()
}

async fn bridge(
    mut downstream: WebSocket,
    state: EdgeState,
    authorization: axum::http::HeaderValue,
) -> FixtureResult {
    let response = state
        .client
        .get(format!("{}/v1/session", state.upstream))
        .header("authorization", authorization)
        .header("Cf-Access-Jwt-Assertion", &state.machine_assertion)
        .upgrade()
        .send()
        .await?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        state
            .rejections
            .lock()
            .map_err(|_| "session rejection lock poisoned")?
            .push(response.status().as_u16());
        state.rejected.notify_one();
    }
    let mut upstream = response.into_websocket().await?;
    state.sessions.fetch_add(1, Ordering::SeqCst);
    loop {
        tokio::select! {
            _ = state.stop.cancelled() => break,
            frame = downstream.recv() => match frame {
                Some(Ok(Message::Binary(bytes))) => upstream.send(UpstreamMessage::Binary(bytes)).await?,
                Some(Ok(Message::Text(text))) => upstream.send(UpstreamMessage::Text(text.to_string())).await?,
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                _ => break,
            },
            frame = upstream.next() => match frame {
                Some(Ok(UpstreamMessage::Binary(bytes))) => downstream.send(Message::Binary(bytes)).await?,
                Some(Ok(UpstreamMessage::Text(text))) => downstream.send(Message::Text(text.into())).await?,
                Some(Ok(UpstreamMessage::Ping(_) | UpstreamMessage::Pong(_))) => {},
                _ => break,
            },
        }
    }
    let _closed = tokio::time::timeout(
        BOUND,
        upstream.close(reqwest_websocket::CloseCode::Normal, None),
    )
    .await;
    let _closed = tokio::time::timeout(BOUND, downstream.close()).await;
    Ok(())
}
