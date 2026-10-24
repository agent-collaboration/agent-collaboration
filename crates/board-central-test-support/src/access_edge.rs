//! The local edge supplies a fixture assertion; central still verifies its signature.
use crate::{BOUND, FixtureResult};
use axum::{
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::IntoResponse,
    routing::get,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message as UpstreamMessage, RequestBuilderExt};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::task::JoinSet;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Clone)]
struct EdgeState {
    upstream: String,
    accepted_sessions: Arc<AtomicUsize>,
    duplicate_mail: Arc<AtomicBool>,
    duplicated_mail: Arc<AtomicUsize>,
    authorization: String,
    assertion: String,
    stop: CancellationToken,
    tasks: TaskTracker,
    claim_grants: crate::claim_grant_drop::ClaimGrantDrop,
    #[cfg(feature = "conformance")]
    piece_integrity: crate::piece_integrity_refusal::PieceIntegrityRefusal,
    lease_notices: crate::lease_notice_gate::LeaseNoticeGate,
    #[cfg(feature = "conformance")]
    payload_inputs: crate::payload_input_observer::PayloadInputObserver,
}
pub(crate) struct SignedAccessEdge {
    pub endpoint: String,
    accepted_sessions: Arc<AtomicUsize>,
    duplicate_mail: Arc<AtomicBool>,
    duplicated_mail: Arc<AtomicUsize>,
    stop: CancellationToken,
    listener: JoinSet<Result<(), std::io::Error>>,
    connections: TaskTracker,
    claim_grants: crate::claim_grant_drop::ClaimGrantDrop,
    #[cfg(feature = "conformance")]
    piece_integrity: crate::piece_integrity_refusal::PieceIntegrityRefusal,
    lease_notices: crate::lease_notice_gate::LeaseNoticeGate,
    #[cfg(feature = "conformance")]
    payload_inputs: crate::payload_input_observer::PayloadInputObserver,
}
impl SignedAccessEdge {
    pub async fn start(upstream: &str, credential: &str) -> FixtureResult<Self> {
        let stop = CancellationToken::new();
        let accepted_sessions = Arc::new(AtomicUsize::new(0));
        let duplicate_mail = Arc::new(AtomicBool::new(false));
        let duplicated_mail = Arc::new(AtomicUsize::new(0));
        let connections = TaskTracker::new();
        let claim_grants = crate::claim_grant_drop::ClaimGrantDrop::default();
        let lease_notices = crate::lease_notice_gate::LeaseNoticeGate::default();
        #[cfg(feature = "conformance")]
        let piece_integrity = crate::piece_integrity_refusal::PieceIntegrityRefusal::default();
        #[cfg(feature = "conformance")]
        let payload_inputs = crate::payload_input_observer::PayloadInputObserver::default();
        let state = EdgeState {
            upstream: format!("{upstream}/v1/session"),
            accepted_sessions: accepted_sessions.clone(),
            duplicate_mail: duplicate_mail.clone(),
            duplicated_mail: duplicated_mail.clone(),
            authorization: format!("Bearer {credential}"),
            assertion: collaboration_central_conformance::machine_assertion(credential)?,
            stop: stop.clone(),
            tasks: connections.clone(),
            claim_grants: claim_grants.clone(),
            #[cfg(feature = "conformance")]
            piece_integrity: piece_integrity.clone(),
            lease_notices: lease_notices.clone(),
            #[cfg(feature = "conformance")]
            payload_inputs: payload_inputs.clone(),
        };
        let app = axum::Router::new()
            .route("/v1/session", get(upgrade))
            .with_state(state);
        let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let endpoint = format!("http://{}/v1/session", socket.local_addr()?);
        let stopped = stop.clone();
        let mut listener = JoinSet::new();
        listener.spawn(async move {
            axum::serve(socket, app)
                .with_graceful_shutdown(stopped.cancelled_owned())
                .await
        });
        Ok(Self {
            endpoint,
            accepted_sessions,
            duplicate_mail,
            duplicated_mail,
            stop,
            listener,
            connections,
            claim_grants,
            #[cfg(feature = "conformance")]
            piece_integrity,
            lease_notices,
            #[cfg(feature = "conformance")]
            payload_inputs,
        })
    }
    #[cfg(feature = "conformance")]
    pub(crate) fn refuse_piece_integrity(
        &self,
        database: fleet_sync_protocol::DatabaseId,
        table: fleet_sync_protocol::TableName,
        references: fleet_sync_protocol::TableName,
    ) -> FixtureResult<crate::PieceIntegrityObservation> {
        self.piece_integrity.begin(database, table, references)
    }
    pub(crate) fn drop_claim_grant_once(
        &self,
        attempt: fleet_sync_protocol::RequestId,
    ) -> FixtureResult<crate::ClaimGrantDropObservation> {
        self.claim_grants.begin(attempt)
    }
    pub(crate) fn hold_granted_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<crate::LeaseNoticeObservation> {
        self.lease_notices.begin(project, epoch)
    }
    pub(crate) fn hold_superseded_notice(
        &self,
        project: message_board::ProjectId,
        epoch: fleet_sync_protocol::Epoch,
    ) -> FixtureResult<crate::LeaseNoticeObservation> {
        self.lease_notices.begin_superseded(project, epoch)
    }
    #[cfg(feature = "conformance")]
    pub(crate) fn observe_payload_input_bodies(
        &self,
    ) -> FixtureResult<crate::PayloadInputObservation> {
        self.payload_inputs.begin()
    }
    pub(crate) fn duplicate_next_mail_notice(&self) {
        self.duplicate_mail.store(true, Ordering::SeqCst);
    }
    pub(crate) fn duplicated_mail_notices(&self) -> usize {
        self.duplicated_mail.load(Ordering::SeqCst)
    }
    pub(crate) fn session_connections(&self) -> usize {
        self.accepted_sessions.load(Ordering::SeqCst)
    }
    pub async fn shutdown(mut self) -> FixtureResult {
        self.stop.cancel();
        self.connections.close();
        while let Some(result) = self.listener.join_next().await {
            result??;
        }
        tokio::time::timeout(BOUND, self.connections.wait()).await?;
        Ok(())
    }
}
impl Drop for SignedAccessEdge {
    fn drop(&mut self) {
        self.stop.cancel();
        self.connections.close();
        self.listener.abort_all();
    }
}
async fn upgrade(
    State(state): State<EdgeState>,
    headers: axum::http::HeaderMap,
    upgrade: WebSocketUpgrade,
) -> axum::response::Response {
    if headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        != Some(state.authorization.as_str())
        || headers
            .get("cf-access-client-id")
            .and_then(|value| value.to_str().ok())
            != Some("board-fixture-client")
        || headers
            .get("cf-access-client-secret")
            .and_then(|value| value.to_str().ok())
            != Some("board-fixture-secret")
    {
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }
    let token = state.tasks.token();
    upgrade
        .on_upgrade(move |socket| async move {
            let _token = token;
            if let Err(error) = bridge(socket, state).await {
                eprintln!("real board fixture Access edge ended: {error}");
            }
        })
        .into_response()
}
async fn bridge(mut downstream: WebSocket, state: EdgeState) -> FixtureResult {
    let mut notice_changes = state.lease_notices.subscribe();
    let mut held_notices: Option<(u64, std::collections::VecDeque<Message>)> = None;
    let mut upstream = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .connect_timeout(BOUND)
        .build()?
        .get(&state.upstream)
        .header("authorization", &state.authorization)
        .header("Cf-Access-Jwt-Assertion", &state.assertion)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    state.accepted_sessions.fetch_add(1, Ordering::SeqCst);
    loop {
        tokio::select! {
            _ = state.stop.cancelled() => break,
            changed = notice_changes.changed() => {
                changed?;
                flush_released_notices(&mut downstream, &state.lease_notices, &mut held_notices).await?;
            },
            frame = downstream.recv() => match frame {
                Some(Ok(Message::Binary(bytes))) => {
                    #[cfg(feature = "conformance")]
                    if let Some(answer) = state.piece_integrity.refuse_request(&bytes)? {
                        downstream.send(Message::Binary(answer.into())).await?;
                        continue;
                    }
                    state.claim_grants.observe_request(&bytes)?;
                    upstream.send(UpstreamMessage::Binary(bytes)).await?;
                },
                Some(Ok(Message::Text(text))) => {
                    #[cfg(feature = "conformance")]
                    if let Some(answer) = state.piece_integrity.refuse_request(text.as_bytes())? {
                        downstream.send(Message::Binary(answer.into())).await?;
                        continue;
                    }
                    state.claim_grants.observe_request(text.as_bytes())?;
                    upstream.send(UpstreamMessage::Text(text.to_string())).await?;
                },
                Some(Ok(Message::Ping(_) | Message::Pong(_))) => {},
                _ => break,
            },
            frame = upstream.next() => match frame {
                Some(Ok(UpstreamMessage::Binary(bytes))) => {
                    if state.claim_grants.drop_answer(&bytes)? { break; }
                    flush_released_notices(&mut downstream, &state.lease_notices, &mut held_notices).await?;
                    if let Some(generation) = state.lease_notices.defer(&bytes)? {
                        queue_held_notice(&mut held_notices, generation, Message::Binary(bytes))?;
                        continue;
                    }
                    #[cfg(feature = "conformance")]
                    state.payload_inputs.observe(&bytes)?;
                    if duplicate_mail_notice(&state,&bytes)? { downstream.send(Message::Binary(bytes.clone())).await?; }
                    downstream.send(Message::Binary(bytes)).await?;
                },
                Some(Ok(UpstreamMessage::Text(text))) => {
                    if state.claim_grants.drop_answer(text.as_bytes())? { break; }
                    flush_released_notices(&mut downstream, &state.lease_notices, &mut held_notices).await?;
                    if let Some(generation) = state.lease_notices.defer(text.as_bytes())? {
                        queue_held_notice(&mut held_notices, generation, Message::Text(text.into()))?;
                        continue;
                    }
                    #[cfg(feature = "conformance")]
                    state.payload_inputs.observe(text.as_bytes())?;
                    if duplicate_mail_notice(&state,text.as_bytes())? { downstream.send(Message::Text(text.clone().into())).await?; }
                    downstream.send(Message::Text(text.into())).await?;
                },
                Some(Ok(UpstreamMessage::Ping(_) | UpstreamMessage::Pong(_))) => {},
                _ => break,
            },
        }
    }
    let _ = tokio::time::timeout(
        BOUND,
        upstream.close(reqwest_websocket::CloseCode::Normal, None),
    )
    .await;
    let _ = tokio::time::timeout(BOUND, downstream.close()).await;
    Ok(())
}

fn queue_held_notice(
    held: &mut Option<(u64, std::collections::VecDeque<Message>)>,
    generation: u64,
    frame: Message,
) -> FixtureResult {
    let (current, frames) =
        held.get_or_insert_with(|| (generation, std::collections::VecDeque::new()));
    if *current != generation || frames.len() >= 64 {
        return Err("grant notice fixture queue exceeded its selected bounded scope".into());
    }
    frames.push_back(frame);
    Ok(())
}

async fn flush_released_notices(
    downstream: &mut WebSocket,
    gate: &crate::lease_notice_gate::LeaseNoticeGate,
    held: &mut Option<(u64, std::collections::VecDeque<Message>)>,
) -> FixtureResult {
    if let Some((generation, _)) = held.as_ref()
        && !gate.is_held(*generation)?
        && let Some((_, mut frames)) = held.take()
    {
        while let Some(frame) = frames.pop_front() {
            downstream.send(frame).await?;
        }
    }
    Ok(())
}

fn duplicate_mail_notice(state: &EdgeState, bytes: &[u8]) -> FixtureResult<bool> {
    if !state.duplicate_mail.load(Ordering::SeqCst) {
        return Ok(false);
    }
    let frame: fleet_sync_protocol::ServerFrame<board_sync_protocol::BoardConsumerProtocol> =
        serde_json::from_slice(bytes)?;
    if matches!(
        frame,
        fleet_sync_protocol::ServerFrame::Notice {
            body: fleet_sync_protocol::Notice::Consumer(
                board_sync_protocol::PositionedBoardNotice {
                    kind: board_sync_protocol::BoardNotice::MailWaiting { .. },
                    ..
                }
            ),
            ..
        }
    ) && state.duplicate_mail.swap(false, Ordering::SeqCst)
    {
        state.duplicated_mail.fetch_add(1, Ordering::SeqCst);
        return Ok(true);
    }
    Ok(false)
}
