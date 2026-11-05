//! Hold one committed Fleet answer while the compiled helper owns offline custody.
use super::offline_cli_fixture::TestResult;
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use std::sync::Arc;
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
};

struct Gate {
    reached: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
#[derive(Clone)]
struct Forwarding {
    upstream: String,
    client: reqwest::Client,
    gate: Arc<Mutex<Option<Gate>>>,
}
pub(super) struct PublicationHold {
    pub endpoint: String,
    reached: oneshot::Receiver<()>,
    release: Option<oneshot::Sender<()>>,
    stop: oneshot::Sender<()>,
    running: JoinHandle<std::io::Result<()>>,
}
impl PublicationHold {
    pub async fn start(upstream: &str) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let (reached, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        let state = Forwarding {
            upstream: upstream.to_owned(),
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(20))
                .build()?,
            gate: Arc::new(Mutex::new(Some(Gate {
                reached,
                release: released,
            }))),
        };
        let app = Router::new()
            .route("/admin/v1/import/fleet", post(forward))
            .route("/admin/v1/import/projects/{project}", post(forward))
            .with_state(state);
        let (stop, stopped) = oneshot::channel();
        let running = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        Ok(Self {
            endpoint,
            reached: observed,
            release: Some(release),
            stop,
            running,
        })
    }
    pub async fn wait_for_committed_fleet(&mut self) -> TestResult {
        tokio::time::timeout(std::time::Duration::from_secs(20), &mut self.reached).await??;
        Ok(())
    }
    pub fn release(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
    pub async fn shutdown(mut self) -> TestResult {
        self.release();
        let _ = self.stop.send(());
        self.running.await??;
        Ok(())
    }
}

async fn forward(
    State(state): State<Forwarding>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match forward_checked(state, uri.path(), headers, body).await {
        Ok(response) => response,
        Err(_) => StatusCode::BAD_GATEWAY.into_response(),
    }
}
async fn forward_checked(
    state: Forwarding,
    path: &str,
    headers: HeaderMap,
    body: Bytes,
) -> TestResult<Response> {
    let mut request = state
        .client
        .post(format!("{}{path}", state.upstream))
        .header("Content-Type", "application/json")
        .body(body);
    for name in ["Cf-Access-Jwt-Assertion", "cf-access-token"] {
        if let Some(value) = headers.get(name) {
            request = request.header(name, value.clone());
        }
    }
    let response = request.send().await?;
    let status = response.status();
    let bytes = response.bytes().await?;
    if path == "/admin/v1/import/fleet" && status.is_success() {
        let gate = state.gate.lock().await.take();
        if let Some(gate) = gate {
            let _ = gate.reached.send(());
            let _ = gate.release.await;
        }
    }
    Ok((status, bytes).into_response())
}
