//! C8 HTTPS stand-in owns discovery, held redemption and refresh-family checks.
use super::{ACCESS, DEVICE, INSTALLATION, ORGANIZATION, PEER, TestResult, fleet_login_https};
use axum::{
    Json, Router,
    extract::{Form, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::sync::{Mutex, Semaphore, mpsc};
use tokio_util::sync::CancellationToken;

pub(super) struct IssuerState {
    pub(super) origin: String,
    pub(super) discovery_reply: DiscoveryReply,
    pub(super) discovery_started: mpsc::UnboundedSender<()>,
    pub(super) release_discovery: Semaphore,
    pub(super) redemption_started: mpsc::UnboundedSender<()>,
    pub(super) release_redemption: Semaphore,
    pub(super) authorizations: AtomicUsize,
    pub(super) refreshes: AtomicUsize,
    pub(super) client: Mutex<Option<String>>,
    pub(super) refresh: Mutex<Option<String>>,
    pub(super) refresh_observed: mpsc::UnboundedSender<HashMap<String, String>>,
}
pub(super) struct Issuer {
    pub(super) address: std::net::SocketAddr,
    pub(super) state: Arc<IssuerState>,
    pub(super) observed_refreshes: mpsc::UnboundedReceiver<HashMap<String, String>>,
    pub(super) observed_discoveries: mpsc::UnboundedReceiver<()>,
    pub(super) observed_redemptions: mpsc::UnboundedReceiver<()>,
    pub(super) stop: CancellationToken,
    pub(super) task: tokio::task::JoinHandle<io::Result<()>>,
}

#[derive(Clone, Copy)]
pub(super) enum DiscoveryReply {
    Complete,
    ExtraClientField,
    IssuerMismatch,
}

impl Issuer {
    pub(super) async fn start(discovery_reply: DiscoveryReply) -> TestResult<Self> {
        let listener = fleet_login_https::HttpsListener::bind().await?;
        let address = listener.address()?;
        let (refresh_observed, observed_refreshes) = mpsc::unbounded_channel();
        let (discovery_started, observed_discoveries) = mpsc::unbounded_channel();
        let (redemption_started, observed_redemptions) = mpsc::unbounded_channel();
        let state = Arc::new(IssuerState {
            origin: format!("https://{address}"),
            discovery_reply,
            discovery_started,
            release_discovery: Semaphore::new(0),
            redemption_started,
            release_redemption: Semaphore::new(0),
            authorizations: AtomicUsize::new(0),
            refreshes: AtomicUsize::new(0),
            client: Mutex::new(None),
            refresh: Mutex::new(None),
            refresh_observed,
        });
        let router = Router::new()
            .route(
                "/.well-known/oauth-authorization-server/api/auth",
                get(metadata),
            )
            .route("/.well-known/fleet-login", get(client_document))
            .route("/device", post(authorize))
            .route("/token", post(token))
            .with_state(state.clone());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Ok(Self {
            address,
            state,
            observed_refreshes,
            observed_discoveries,
            observed_redemptions,
            stop,
            task,
        })
    }
    pub(super) fn sign_in(&self) -> String {
        format!("https://{}/api/auth", self.address)
    }
    pub(super) async fn shutdown(self) -> TestResult {
        self.stop.cancel();
        self.task.await??;
        Ok(())
    }
}

async fn metadata(State(state): State<Arc<IssuerState>>) -> Response {
    let _sent = state.discovery_started.send(());
    let Ok(released) = state.release_discovery.acquire().await else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    released.forget();
    let issuer = match state.discovery_reply {
        DiscoveryReply::IssuerMismatch => format!("{}/other-issuer", state.origin),
        DiscoveryReply::Complete | DiscoveryReply::ExtraClientField => {
            format!("{}/api/auth", state.origin)
        }
    };
    Json(json!({"issuer":issuer,"device_authorization_endpoint":format!("{}/device", state.origin),"token_endpoint":format!("{}/token", state.origin)})).into_response()
}

async fn client_document(State(state): State<Arc<IssuerState>>) -> Json<Value> {
    Json(match state.discovery_reply {
        DiscoveryReply::ExtraClientField => {
            json!({"client_id":"fixture-public-device-client","unexpected":true})
        }
        DiscoveryReply::Complete | DiscoveryReply::IssuerMismatch => {
            json!({"client_id":"fixture-public-device-client"})
        }
    })
}
async fn authorize(
    State(state): State<Arc<IssuerState>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if form.get("organization_id").map(String::as_str) != Some(ORGANIZATION)
        || form.get("machine_installation_id").map(String::as_str) != Some(INSTALLATION)
        || form.get("machine_peer_id").map(String::as_str) != Some(PEER)
        || form.get("machine_label").map(String::as_str) != Some("CLI fixture machine")
        || form.get("client_id").map(String::as_str) != Some("fixture-public-device-client")
        || form.get("scope").map(String::as_str) != Some("offline_access")
        || form.contains_key("resource")
        || form.len() != 6
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response();
    }
    state.authorizations.fetch_add(1, Ordering::SeqCst);
    Json(json!({"device_code":DEVICE,"user_code":"ABCD-EFGH","verification_uri":"https://sign-in.example.invalid/activate","verification_uri_complete":"https://sign-in.example.invalid/activate?user_code=full-url-secret-canary","expires_in":600,"interval":5})).into_response()
}
async fn token(
    State(state): State<Arc<IssuerState>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    if form.contains_key("resource") {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response();
    }
    match form.get("grant_type").map(String::as_str) {
        Some("urn:ietf:params:oauth:grant-type:device_code")
            if form.get("device_code").map(String::as_str) == Some(DEVICE)
                && form.get("client_id").map(String::as_str)
                    == Some("fixture-public-device-client") =>
        {
            let _sent = state.redemption_started.send(());
            let Ok(released) = state.release_redemption.acquire().await else {
                return StatusCode::SERVICE_UNAVAILABLE.into_response();
            };
            released.forget();
            let ordinal = 0x200 + state.authorizations.load(Ordering::SeqCst);
            let client = format!("019f0000-0000-7000-8000-{ordinal:012x}");
            let refresh = format!("f11-refresh-secret-canary-{ordinal}");
            *state.client.lock().await = Some(client.clone());
            *state.refresh.lock().await = Some(refresh.clone());
            Json(json!({"access_token":ACCESS,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"machine_client_id":client,
                "fleet_central":{"receiver":"019f0000-0000-7000-8000-000000000105","relay":"019f0000-0000-7000-8000-000000000106"}})).into_response()
        }
        Some("refresh_token") => {
            let client = state.client.lock().await.clone();
            let mut current = state.refresh.lock().await;
            if form.get("client_id") != client.as_ref()
                || form.get("refresh_token") != current.as_ref()
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error":"invalid_grant"})),
                )
                    .into_response();
            }
            let ordinal = state.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            let refresh = format!("f11-rotated-refresh-secret-canary-{ordinal}");
            *current = Some(refresh.clone());
            let _sent = state.refresh_observed.send(form);
            Json(json!({"access_token":ACCESS,"refresh_token":refresh,"token_type":"Bearer","expires_in":600,"machine_client_id":client})).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"invalid_request"})),
        )
            .into_response(),
    }
}
