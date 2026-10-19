//! Test-only wire observer forwards every call to the genuine API unchanged.
//! It records actual method arguments and never manufactures a tool response or provider rejection.
use axum::{Router, body::Body, extract::State, response::Response, routing::post};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

struct RecorderState {
    api_url: String,
    client: reqwest::Client,
    requests: Mutex<Vec<serde_json::Value>>,
}

#[allow(
    dead_code,
    reason = "TCP and Unix observer entrypoints are shared by separate test targets"
)]
pub(super) struct RecordedApiListener {
    pub(super) url: Option<String>,
    state: Arc<RecorderState>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

#[allow(
    dead_code,
    reason = "TCP and Unix observer entrypoints are shared by separate test targets"
)]
impl RecordedApiListener {
    pub(super) async fn start(api_url: String, shutdown: CancellationToken) -> Self {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let state = Arc::new(RecorderState {
            api_url,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            requests: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/mcp", post(record_call))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Self {
            url: Some(format!("http://{address}/mcp")),
            state,
            task,
        }
    }

    pub(super) async fn start_on_unix(
        api_url: String,
        path: &std::path::Path,
        shutdown: CancellationToken,
    ) -> Self {
        let listener = tokio::net::UnixListener::bind(path).unwrap();
        let state = Arc::new(RecorderState {
            api_url,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(12))
                .build()
                .unwrap(),
            requests: Mutex::new(Vec::new()),
        });
        let router = Router::new()
            .route("/mcp", post(record_call))
            .with_state(Arc::clone(&state));
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        Self {
            url: None,
            state,
            task,
        }
    }

    pub(super) fn arguments_for(&self, method: &str) -> Vec<serde_json::Value> {
        self.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| {
                request
                    .pointer("/params/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(method)
            })
            .filter_map(|request| request.pointer("/params/arguments").cloned())
            .collect()
    }

    pub(super) fn call_count(&self, method: &str) -> usize {
        self.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|observed| {
                observed
                    .pointer("/params/name")
                    .and_then(serde_json::Value::as_str)
                    == Some(method)
            })
            .count()
    }

    pub(super) async fn finish(self) {
        tokio::time::timeout(Duration::from_secs(2), self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}

async fn record_call(
    State(state): State<Arc<RecorderState>>,
    request: axum::extract::Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let bytes = axum::body::to_bytes(
        body,
        collaboration_client::protocol::MAX_CONTROL_FRAME_BYTES,
    )
    .await
    .unwrap();
    let message: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    if message
        .pointer("/params/name")
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        state.requests.lock().unwrap().push(message);
    }
    let mut forwarded = state.client.post(&state.api_url);
    for header in [
        reqwest::header::CONTENT_TYPE,
        reqwest::header::ACCEPT,
        reqwest::header::HeaderName::from_static("mcp-protocol-version"),
    ] {
        if let Some(value) = parts.headers.get(&header) {
            forwarded = forwarded.header(header, value);
        }
    }
    let response = forwarded.body(bytes).send().await.unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .cloned();
    let body = response.bytes().await.unwrap();
    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(reqwest::header::CONTENT_TYPE, content_type);
    }
    response.body(Body::from(body)).unwrap()
}
