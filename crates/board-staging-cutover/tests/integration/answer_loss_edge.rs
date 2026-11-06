//! Suppress one successful real Central answer after its HTTP request committed.
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
    routing::post,
};
use collaboration_central_core::{AdminProjectImportAnswer, AdminProjectImportRequest};
use std::sync::{Arc, Mutex};
use tokio::{sync::oneshot, task::JoinHandle};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[derive(Clone, Copy, Debug)]
pub enum LostAnswer {
    Begin,
    Page,
    Complete,
}
impl LostAnswer {
    fn matches(self, request: &AdminProjectImportRequest) -> bool {
        matches!(
            (self, request),
            (Self::Begin, AdminProjectImportRequest::Begin { .. })
                | (Self::Page, AdminProjectImportRequest::Page { page: 0, .. })
                | (Self::Complete, AdminProjectImportRequest::Complete { .. })
        )
    }
}

#[derive(Clone)]
struct EdgeState {
    upstream: String,
    client: reqwest::Client,
    selected: LostAnswer,
    suppressed: Arc<Mutex<Option<AdminProjectImportAnswer>>>,
}

pub struct AnswerLossEdge {
    pub endpoint: String,
    suppressed: Arc<Mutex<Option<AdminProjectImportAnswer>>>,
    stop: oneshot::Sender<()>,
    running: JoinHandle<std::io::Result<()>>,
}
impl AnswerLossEdge {
    pub async fn start(upstream: &str, selected: LostAnswer) -> TestResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}", listener.local_addr()?);
        let suppressed = Arc::new(Mutex::new(None));
        let state = EdgeState {
            upstream: upstream.to_owned(),
            client: reqwest::Client::builder()
                .no_proxy()
                .timeout(std::time::Duration::from_secs(20))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            selected,
            suppressed: suppressed.clone(),
        };
        let app = Router::new()
            .route("/admin/v1/import/fleet", post(forward))
            .route("/admin/v1/import/projects/{project}", post(forward))
            .with_state(state);
        let (stop, stopped) = oneshot::channel();
        let running = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _stopped = stopped.await;
                })
                .await
        });
        Ok(Self {
            endpoint,
            suppressed,
            stop,
            running,
        })
    }
    pub fn suppressed(&self) -> TestResult<AdminProjectImportAnswer> {
        self.suppressed
            .lock()
            .map_err(|_| "answer observation poisoned")?
            .clone()
            .ok_or_else(|| "successful Central answer was not suppressed".into())
    }
    pub async fn shutdown(self) -> TestResult {
        let _sent = self.stop.send(());
        self.running.await??;
        Ok(())
    }
}

async fn forward(
    State(state): State<EdgeState>,
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
    state: EdgeState,
    path: &str,
    headers: HeaderMap,
    body: Bytes,
) -> TestResult<Response> {
    let request = serde_json::from_slice::<AdminProjectImportRequest>(&body).ok();
    let mut forwarded = state
        .client
        .post(format!("{}{path}", state.upstream))
        .header("Content-Type", "application/json")
        .body(body);
    for name in ["Cf-Access-Jwt-Assertion", "cf-access-token"] {
        if let Some(value) = headers.get(name) {
            forwarded = forwarded.header(name, value.clone());
        }
    }
    let response = forwarded.send().await?;
    let status = response.status();
    let body = response.bytes().await?;
    if status.is_success()
        && request
            .as_ref()
            .is_some_and(|request| state.selected.matches(request))
    {
        let answer: AdminProjectImportAnswer = serde_json::from_slice(&body)?;
        let successful = matches!(
            answer,
            AdminProjectImportAnswer::Ready { .. }
                | AdminProjectImportAnswer::Staged { .. }
                | AdminProjectImportAnswer::Imported { .. }
        );
        let mut captured = state
            .suppressed
            .lock()
            .map_err(|_| "answer observation poisoned")?;
        if successful && captured.is_none() {
            *captured = Some(answer);
            // The caller gets no Central answer; its commit is already durable.
            return Ok(StatusCode::BAD_GATEWAY.into_response());
        }
    }
    Ok((status, [("Content-Type", "application/json")], body).into_response())
}
