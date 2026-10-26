//! Local Access edge: signs fixture assertions and can hold one real read frame.
use super::*;
use axum::{
    extract::{
        State,
        ws::{Message as EdgeMessage, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use board_sync_protocol::{BoardReadCursor, BoardReadScope};
use std::collections::VecDeque;
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Debug, PartialEq)]
pub enum ReadPoint {
    SnapshotContinuation,
    FeedAfter(u64),
    FeedReplyThrough(u64),
    EmptyFeedReply,
}
struct WaitingRead {
    point: ReadPoint,
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
#[derive(Clone)]
pub struct Controls {
    reads: Arc<Mutex<VecDeque<WaitingRead>>>,
    malformed_archive: Arc<std::sync::atomic::AtomicBool>,
}
pub struct HeldRead {
    entered: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
}
impl HeldRead {
    pub async fn entered(&mut self) -> TestResult {
        tokio::time::timeout(BOUND, &mut self.entered).await??;
        Ok(())
    }
    pub fn release(self) -> TestResult {
        self.release.send(()).map_err(|_| "held read ended".into())
    }
    pub fn is_held(&self) -> bool {
        !self.release.is_closed()
    }
}
impl Controls {
    pub fn corrupt_next_archive_outcome(&self) {
        self.malformed_archive
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    async fn after_response(&self, bytes: &[u8]) -> TestResult<Option<Vec<u8>>> {
        use board_sync_protocol::{BoardConsumerAnswer, BoardPayload};
        use fleet_sync_protocol::{OperationAnswer, ServerFrame};
        let answer: ServerFrame<BoardConsumerProtocol> = serde_json::from_slice(bytes)?;
        if let ServerFrame::Answer {
            body:
                OperationAnswer::Consumer(BoardConsumerAnswer::Read(
                    board_sync_protocol::BoardReadPage::FleetFeed(
                        board_sync_protocol::FleetFeedRead::Page {
                            records,
                            through,
                            head,
                            ..
                        },
                    ),
                )),
            ..
        } = &answer
        {
            let mut points = vec![ReadPoint::FeedReplyThrough(through.get())];
            if records.is_empty() && through == head {
                points.push(ReadPoint::EmptyFeedReply);
            }
            self.wait_at_matching(&points).await?;
        }
        if let ServerFrame::Answer {
            frame,
            body: OperationAnswer::Consumer(BoardConsumerAnswer::ArchiveSeen(_)),
        } = answer
            && self
                .malformed_archive
                .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(Some(serde_json::to_vec(&ServerFrame::<
                BoardConsumerProtocol,
            >::Answer {
                frame,
                body: OperationAnswer::Consumer(BoardConsumerAnswer::ArchiveSeen(
                    BoardPayload::encode(&serde_json::json!({"kind":"inventedArchiveOutcome"}))?,
                )),
            })?));
        }
        Ok(None)
    }
    pub async fn hold(&self, point: ReadPoint) -> HeldRead {
        let (entered, waiting) = oneshot::channel();
        let (release, released) = oneshot::channel();
        self.reads.lock().await.push_back(WaitingRead {
            point,
            entered,
            release: released,
        });
        HeldRead {
            entered: waiting,
            release,
        }
    }
    async fn before_forward(&self, bytes: &[u8]) -> TestResult {
        let point = match serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(bytes)? {
            ClientFrame::Request {
                body:
                    OperationRequest::Consumer(BoardConsumerRequest::Read {
                        scope: BoardReadScope::FleetSnapshot,
                        cursor: Some(BoardReadCursor::FleetSnapshot(_)),
                    }),
                ..
            } => Some(ReadPoint::SnapshotContinuation),
            ClientFrame::Request {
                body:
                    OperationRequest::Consumer(BoardConsumerRequest::Read {
                        scope: BoardReadScope::FleetRecords,
                        cursor: Some(BoardReadCursor::Fleet(position)),
                    }),
                ..
            } => Some(ReadPoint::FeedAfter(position.get())),
            _ => None,
        };
        if let Some(point) = point {
            self.wait_at_matching(&[point]).await?;
        }
        Ok(())
    }
    async fn wait_at_matching(&self, points: &[ReadPoint]) -> TestResult {
        let waiting = {
            let mut reads = self.reads.lock().await;
            if reads
                .front()
                .is_some_and(|read| points.contains(&read.point))
            {
                reads.pop_front()
            } else {
                None
            }
        };
        if let Some(read) = waiting {
            read.entered.send(()).map_err(|_| "read observer ended")?;
            tokio::time::timeout(BOUND, read.release).await??;
        }
        Ok(())
    }
}
#[derive(Clone)]
struct EdgeState {
    upstream: String,
    controls: Controls,
    stop: CancellationToken,
}
pub struct Ingress {
    pub endpoint: String,
    pub controls: Controls,
    stop: CancellationToken,
    task: tokio::task::JoinHandle<Result<(), std::io::Error>>,
}
impl Ingress {
    pub async fn start(upstream: &str) -> TestResult<Self> {
        let stop = CancellationToken::new();
        let controls = Controls {
            reads: Arc::new(Mutex::new(VecDeque::new())),
            malformed_archive: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let state = EdgeState {
            upstream: format!("{upstream}/v1/session"),
            controls: controls.clone(),
            stop: stop.clone(),
        };
        let app = axum::Router::new()
            .route("/v1/session", get(upgrade))
            .with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/v1/session", listener.local_addr()?);
        let stopped = stop.clone();
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            endpoint,
            controls,
            stop,
            task,
        })
    }
    pub async fn shutdown(self) -> TestResult {
        self.stop.cancel();
        self.task.await??;
        Ok(())
    }
}
async fn upgrade(
    State(state): State<EdgeState>,
    headers: axum::http::HeaderMap,
    upgrade: WebSocketUpgrade,
) -> axum::response::Response {
    if headers
        .get("cf-access-client-id")
        .and_then(|value| value.to_str().ok())
        != Some("cache-fixture-client")
        || headers
            .get("cf-access-client-secret")
            .and_then(|value| value.to_str().ok())
            != Some("cache-fixture-secret")
        || headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            != Some("Bearer cache-reader")
    {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    upgrade
        .on_upgrade(move |socket| async move {
            if let Err(error) = bridge(socket, state).await {
                eprintln!("cache fixture ingress ended: {error}");
            }
        })
        .into_response()
}
async fn bridge(mut downstream: axum::extract::ws::WebSocket, state: EdgeState) -> TestResult {
    use reqwest_websocket::RequestBuilderExt;
    let mut upstream = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .build()?
        .get(&state.upstream)
        .bearer_auth("cache-reader")
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion("cache-reader")?,
        )
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    loop {
        tokio::select! {
            _ = state.stop.cancelled() => break,
            message = downstream.recv() => {
                let bytes = match message {
                    Some(Ok(EdgeMessage::Binary(bytes))) => bytes,
                    Some(Ok(EdgeMessage::Text(text))) => text.as_bytes().to_vec().into(),
                    Some(Ok(EdgeMessage::Ping(_) | EdgeMessage::Pong(_))) => continue,
                    _ => break,
                };
                state.controls.before_forward(&bytes).await?;
                upstream.send(Message::Binary(bytes)).await?;
            }
            message = upstream.next() => {
                let bytes = match message {
                    Some(Ok(Message::Binary(bytes))) => bytes,
                    Some(Ok(Message::Text(text))) => text.into_bytes().into(),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break,
                };
                let bytes = state.controls.after_response(&bytes).await?.map_or(bytes, Into::into);
                downstream.send(EdgeMessage::Binary(bytes)).await?;
            }
        }
    }
    let _ = upstream
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    let _ = downstream.close().await;
    Ok(())
}
