//! The one task that owns the Router's central link: the C1 link client, the
//! socket, the outbox flush reducer and the calls waiting on them. Every
//! deadline comes from the one injected clock.
use super::link_transport::{
    LinkClosed, LinkInbound, LinkSocket, LinkTransport, LinkTransportError, MachineTokenSource,
    TokenAvailability, TransportFuture,
};
use super::{DriverCommand, FlushJitter};
use crate::OutboxStorage;
use crate::outbox_store::{NewOutboxItem, OutboxInsert, OutboxStoreError};
use central_link_client::{
    CallFailure, ClientAction, ClientEvent, ConnectionAttempt, FailedCall, FlushEvent,
    FlushOutcome, FlushOutcomeSink, LinkAdmissionRefusal, LinkClient, MemberOutcome,
    OutboxFlushState, OutboxReceipt, PendingWrite, RefusalReason as LinkRefusal, RequestStatus,
    Submission, SubmissionPath, SubmitRefusal, submission_path,
};
use central_link_protocol::{
    CorrelationId, DataFrame, FrameDirection, LinkFrame, LinkRequest, MachineTokenText,
    RequestPayload, RequestScope, decode_text, encode_text,
};
use router_clock::{ClockInstant, RuntimeClock};
use runtime_primitives::SyncTime;
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use workspace_model::{Caller, RequestId};

/// An immediate call waiting for central's reply.
struct ImmediateCall {
    request: Option<RequestId>,
    scope: RequestScope,
    reply: oneshot::Sender<Submission>,
}

/// The flush batch on the wire: each member's correlation, id and caller.
struct BatchInFlight {
    mark: SyncTime,
    members: BTreeMap<CorrelationId, (RequestId, Caller)>,
    /// Every member's caller for the whole batch, for the outcome report.
    callers: BTreeMap<RequestId, Caller>,
    outcomes: Vec<FlushOutcome>,
    /// Some member went back to the outbox unapplied; the report is incomplete.
    returned: bool,
}

type PendingReply = oneshot::Sender<Result<Vec<PendingWrite>, OutboxStoreError>>;

pub(super) struct LinkDriver {
    pub(super) clock: Arc<dyn RuntimeClock>,
    pub(super) store: Arc<Mutex<dyn OutboxStorage>>,
    pub(super) transport: Arc<dyn LinkTransport>,
    pub(super) tokens: Arc<dyn MachineTokenSource>,
    pub(super) jitter: Arc<dyn FlushJitter>,
    pub(super) sink: Arc<dyn FlushOutcomeSink>,
    pub(super) client: LinkClient,
    pub(super) flush: OutboxFlushState,
    socket: Option<(ConnectionAttempt, Box<dyn LinkSocket>)>,
    /// A connect in progress. It is polled in the run loop, so the driver keeps
    /// serving calls, and the client's Welcome deadline abandons it.
    connecting: Option<PendingConnect>,
    immediate: BTreeMap<CorrelationId, ImmediateCall>,
    batch: Option<BatchInFlight>,
    flush_wake: Option<ClockInstant>,
    deferred_pending: Vec<(Caller, PendingReply)>,
    /// The token the open connection was admitted or last renewed with.
    connected_token: Option<MachineTokenText>,
    /// A RenewMachineToken on the wire and the token it carries.
    renewing: Option<(CorrelationId, MachineTokenText)>,
    renewal_ids: uuid::ContextV7,
    /// When a renewal central could not answer (CentralUnavailable) is tried again.
    renew_retry_at: Option<ClockInstant>,
    /// The token whose connection central closed as authority lapsed; the link
    /// waits for the source to offer a different one before connecting again.
    lapsed_token: Option<MachineTokenText>,
}

impl LinkDriver {
    pub(super) fn new(
        clock: Arc<dyn RuntimeClock>,
        store: Arc<Mutex<dyn OutboxStorage>>,
        transport: Arc<dyn LinkTransport>,
        tokens: Arc<dyn MachineTokenSource>,
        jitter: Arc<dyn FlushJitter>,
        sink: Arc<dyn FlushOutcomeSink>,
        client: LinkClient,
    ) -> Self {
        Self {
            clock,
            store,
            transport,
            tokens,
            jitter,
            sink,
            client,
            flush: OutboxFlushState::default(),
            socket: None,
            connecting: None,
            immediate: BTreeMap::new(),
            batch: None,
            flush_wake: None,
            deferred_pending: Vec::new(),
            connected_token: None,
            renewing: None,
            renewal_ids: uuid::ContextV7::new(),
            renew_retry_at: None,
            lapsed_token: None,
        }
    }

    pub(super) async fn run(
        mut self,
        mut commands: mpsc::Receiver<DriverCommand>,
        stop: CancellationToken,
    ) {
        // Start-up: sent rows from before a stop go back to waiting.
        self.reconcile_sent().await;
        loop {
            self.drive_time().await;
            let deadline = [
                self.client.next_deadline(),
                self.flush_wake,
                self.renew_retry_at,
            ]
            .into_iter()
            .flatten()
            .min();
            let clock = self.clock.clone();
            let tokens = self.tokens.clone();
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                () = tokens.changed() => self.token_changed().await,
                command = commands.recv() => match command {
                    Some(command) => self.handle_command(command).await,
                    None => break,
                },
                connected = pending_connect(&mut self.connecting) => self.connect_finished(connected).await,
                inbound = next_inbound(&mut self.socket) => self.handle_inbound(inbound).await,
                () = sleep_until(clock.as_ref(), deadline) => {}
            }
        }
        if let Some((_, mut socket)) = self.socket.take() {
            socket.close().await;
        }
    }

    async fn drive_time(&mut self) {
        while let Some(action) = self.client.poll_time() {
            self.handle_action(action).await;
        }
        self.drive_renewal_retry().await;
        if self
            .flush_wake
            .is_some_and(|wake| self.clock.monotonic_now() >= wake)
        {
            self.flush_wake = None;
            let now = self.clock.now();
            self.flush_tick(now).await;
        }
    }

    async fn handle_action(&mut self, action: ClientAction) {
        match action {
            ClientAction::Connect { attempt } => self.connect(attempt).await,
            ClientAction::SendPing => {
                if !self.send(&LinkFrame::Ping).await {
                    self.socket_closed().await;
                }
            }
            ClientAction::Reconnect { failed } => {
                // The Welcome deadline passed: abandon a connect still in progress.
                self.connecting = None;
                if let Some((_, mut socket)) = self.socket.take() {
                    socket.close().await;
                }
                self.settle_failed(failed).await;
                self.return_batch().await;
                self.apply_flush(FlushEvent::LinkLost).await;
            }
            ClientAction::CallsExpired { failed } => self.settle_failed(failed).await,
        }
    }

    async fn connect(&mut self, attempt: ConnectionAttempt) {
        let token = match self.tokens.availability() {
            TokenAvailability::Ready(token) if self.lapsed_token.as_ref() != Some(&token) => token,
            // The lapsed token cannot connect again: wait, as for a refresh.
            TokenAvailability::Ready(_) | TokenAvailability::RefreshPending => {
                if let Ok(failed) = self.client.closed(&attempt) {
                    self.settle_failed(failed).await;
                }
                return;
            }
            // Not enrolled; new code needed: stop retrying until the source changes.
            TokenAvailability::NeedsNewCode => {
                if let Ok(failed) = self
                    .client
                    .refused(&attempt, LinkRefusal::CredentialRefused)
                {
                    self.settle_failed(failed).await;
                }
                return;
            }
        };
        let transport = self.transport.clone();
        let dialing = token.clone();
        self.connecting = Some(PendingConnect {
            attempt,
            token,
            connect: Box::pin(async move { transport.connect(&dialing).await }),
        });
    }

    async fn connect_finished(
        &mut self,
        (attempt, token, socket): (ConnectionAttempt, MachineTokenText, ConnectResult),
    ) {
        match socket {
            Ok(socket) => {
                self.socket = Some((attempt, socket));
                self.connected_token = Some(token);
            }
            Err(_) => {
                if let Ok(failed) = self.client.closed(&attempt) {
                    self.settle_failed(failed).await;
                }
            }
        }
    }

    async fn socket_closed(&mut self) {
        let Some((attempt, mut socket)) = self.socket.take() else {
            return;
        };
        socket.close().await;
        if let Ok(failed) = self.client.closed(&attempt) {
            self.settle_failed(failed).await;
        }
        self.return_batch().await;
        self.apply_flush(FlushEvent::LinkLost).await;
    }

    async fn handle_inbound(&mut self, inbound: LinkInbound) {
        let text = match inbound {
            LinkInbound::Text(text) => text,
            // Central refused a frame past the deadline or after revocation and
            // closed: reconnect only with a fresh token.
            LinkInbound::Closed(LinkClosed::AuthorityLapsed) => {
                self.lapsed_token = self.connected_token.take();
                self.socket_closed().await;
                return;
            }
            LinkInbound::Closed(LinkClosed::Other) => {
                self.socket_closed().await;
                return;
            }
        };
        let Some((attempt, _)) = &self.socket else {
            return;
        };
        let attempt = attempt.clone();
        let event = match decode_text(&text, FrameDirection::ToRouter)
            .ok()
            .map(|frame| self.client.receive(&attempt, frame))
        {
            Some(Ok(event)) => event,
            Some(Err(_)) | None => {
                self.socket_closed().await;
                return;
            }
        };
        match event {
            ClientEvent::Welcomed => {
                self.lapsed_token = None;
                self.flush_link_connected().await;
            }
            ClientEvent::Reply {
                correlation_id,
                result,
            } => self.reply_arrived(&correlation_id, result).await,
            // Pong resets the client's heartbeat; a late reply was settled.
            // Inbox delivery and message outcomes arrive in 2c steps C and D.
            ClientEvent::TokenRenewal {
                correlation_id,
                result,
            } => self.renewal_answered(&correlation_id, &result),
            ClientEvent::Pong | ClientEvent::LateReply { .. } | ClientEvent::Deliver { .. } => {}
        }
    }

    async fn reply_arrived(
        &mut self,
        correlation: &CorrelationId,
        result: central_link_protocol::ReplyPayload,
    ) {
        if let Some(call) = self.immediate.remove(correlation) {
            let _ = call.reply.send(Submission::Answered(result));
            return;
        }
        let member = self
            .batch
            .as_mut()
            .and_then(|batch| batch.members.remove(correlation));
        if let Some((request, _)) = member {
            self.batch_member_answered(request, MemberOutcome::Answered(result))
                .await;
        }
    }

    async fn settle_failed(&mut self, failed: Vec<FailedCall>) {
        let mut batch_member_failed = false;
        for call in failed {
            if let Some(immediate) = self.immediate.remove(&call.correlation_id) {
                // The call was accepted, so its frame may have left: a read is
                // ReadAnswerLost; a write the client knows was never sent is unavailable.
                let submission = match (call.failure, immediate.request) {
                    (CallFailure::CentralUnavailable, None) => Submission::ReadAnswerLost,
                    (CallFailure::CentralUnavailable, Some(_)) => {
                        Submission::Refused(SubmitRefusal::CentralUnavailable)
                    }
                    (CallFailure::OutcomeUnknown { request_id }, _) => Submission::Uncertain {
                        request: request_id,
                        scope: immediate.scope,
                    },
                };
                let _ = immediate.reply.send(submission);
            } else if self
                .batch
                .as_ref()
                .is_some_and(|batch| batch.members.contains_key(&call.correlation_id))
            {
                batch_member_failed = true;
            }
        }
        if batch_member_failed {
            self.return_batch().await;
        }
    }

    async fn handle_command(&mut self, command: DriverCommand) {
        match command {
            DriverCommand::Submit {
                caller,
                payload,
                reply,
            } => match submission_path(&payload) {
                SubmissionPath::Outbox => {
                    if let Some(submission) = self.submit_to_outbox(caller, payload, &reply).await {
                        let _ = reply.send(submission);
                    }
                }
                SubmissionPath::Immediate if self.not_enrolled() => {
                    let _ = reply.send(Submission::Refused(SubmitRefusal::NotEnrolled));
                }
                SubmissionPath::Immediate => self.submit_now(caller, payload, reply).await,
            },
            DriverCommand::Status {
                caller,
                request,
                reply,
            } => {
                let mut store = self.store.lock().await;
                let status = store
                    .request_status(&caller, &request)
                    .await
                    .unwrap_or(RequestStatus::NotFound);
                let _ = reply.send(status);
            }
            DriverCommand::Pending { caller, reply } => {
                if self.batch.is_some() {
                    self.deferred_pending.push((caller, reply));
                } else {
                    let pending = pending_for(&self.store, &caller).await;
                    let _ = reply.send(pending);
                }
            }
            DriverCommand::LinkStatus { reply } => {
                let _ = reply.send(self.client.status());
            }
            DriverCommand::Outgoing {
                caller,
                limit,
                reply,
            } => {
                let mut store = self.store.lock().await;
                let _ = reply.send(store.outgoing_for(&caller, limit).await);
            }
        }
    }

    /// Stores an outbox write and answers with its receipt. None when the caller
    /// went away before admission: no row is written and nothing is sent
    /// (pre-admission cancel). Once the insert commits, nothing is undone.
    async fn submit_to_outbox(
        &mut self,
        caller: Caller,
        payload: RequestPayload,
        reply: &oneshot::Sender<Submission>,
    ) -> Option<Submission> {
        let Some(request) = payload.write_request_id().cloned() else {
            return Some(Submission::Refused(SubmitRefusal::OutboxUnavailable));
        };
        let now = self.clock.now();
        let flush_at = self.flush.receipt_mark(now);
        let item = NewOutboxItem {
            request: request.clone(),
            caller,
            payload,
            flush_at,
        };
        let inserted = {
            let mut store = self.store.lock().await;
            if reply.is_closed() {
                return None;
            }
            store.insert(&item, now).await
        };
        Some(match inserted {
            Ok(OutboxInsert::Stored) => {
                self.apply_flush(FlushEvent::Stored { flush_at }).await;
                Submission::InOutbox(OutboxReceipt {
                    id: request,
                    flush_at,
                })
            }
            Ok(OutboxInsert::AlreadyStored(status)) => match status {
                RequestStatus::InOutbox { flush_at } => Submission::InOutbox(OutboxReceipt {
                    id: request,
                    flush_at,
                }),
                RequestStatus::Answered(reply) => Submission::Answered(reply),
                RequestStatus::Sent | RequestStatus::NotFound => Submission::Uncertain {
                    request,
                    scope: item.payload.request_scope(),
                },
                RequestStatus::Refused(LinkAdmissionRefusal::RequestIdConflict) => {
                    Submission::Refused(SubmitRefusal::RequestIdConflict)
                }
                RequestStatus::Refused(_) | RequestStatus::Cancelled { .. } => {
                    Submission::Uncertain {
                        request,
                        scope: item.payload.request_scope(),
                    }
                }
            },
            Ok(OutboxInsert::ConflictingContent) => {
                Submission::Refused(SubmitRefusal::RequestIdConflict)
            }
            Err(error) => {
                // The insert may have committed with its acknowledgement lost;
                // the store is the truth, so arm its mark anyway. If nothing
                // was stored, that tick finds nothing due.
                tracing::warn!(%error, "outbox: storing a write failed");
                self.apply_flush(FlushEvent::Stored { flush_at }).await;
                Submission::Refused(SubmitRefusal::OutboxUnavailable)
            }
        })
    }

    async fn submit_now(
        &mut self,
        caller: Caller,
        payload: RequestPayload,
        reply: oneshot::Sender<Submission>,
    ) {
        // Pre-admission cancel: a caller already gone gets no frame. From here to
        // the send nothing awaits, so this is the last point before the frame.
        if reply.is_closed() {
            return;
        }
        let request = payload.write_request_id().cloned();
        let scope = payload.request_scope();
        let frame = match self.client.begin_request(caller, payload) {
            Ok(frame) => frame,
            Err(_) => {
                let _ = reply.send(Submission::Refused(SubmitRefusal::CentralUnavailable));
                return;
            }
        };
        let Some(correlation) = request_correlation(&frame) else {
            let _ = reply.send(Submission::Refused(SubmitRefusal::CentralUnavailable));
            return;
        };
        self.immediate.insert(
            correlation,
            ImmediateCall {
                request,
                scope,
                reply,
            },
        );
        if !self.send(&frame).await {
            self.socket_closed().await;
        }
    }

    async fn send(&mut self, frame: &LinkFrame) -> bool {
        let Some((_, socket)) = self.socket.as_mut() else {
            return false;
        };
        let Ok(text) = encode_text(frame, FrameDirection::ToCentral) else {
            return false;
        };
        socket.send_text(text).await.is_ok()
    }
}

fn request_correlation(frame: &LinkFrame) -> Option<CorrelationId> {
    match frame {
        LinkFrame::Data(DataFrame::Request { request }) => {
            Some(LinkRequest::correlation_id(request).clone())
        }
        LinkFrame::Ping | LinkFrame::Pong | LinkFrame::Data(_) => None,
    }
}

type ConnectResult = Result<Box<dyn LinkSocket>, LinkTransportError>;

/// A connect started for one attempt, with the token it dials.
struct PendingConnect {
    attempt: ConnectionAttempt,
    token: MachineTokenText,
    connect: TransportFuture<'static, ConnectResult>,
}

/// Resolves when the pending connect finishes; never while none is pending.
async fn pending_connect(
    connecting: &mut Option<PendingConnect>,
) -> (ConnectionAttempt, MachineTokenText, ConnectResult) {
    let Some(pending) = connecting.as_mut() else {
        return std::future::pending().await;
    };
    let socket = pending.connect.as_mut().await;
    let Some(PendingConnect { attempt, token, .. }) = connecting.take() else {
        return std::future::pending().await;
    };
    (attempt, token, socket)
}

async fn next_inbound(
    socket: &mut Option<(ConnectionAttempt, Box<dyn LinkSocket>)>,
) -> LinkInbound {
    match socket {
        Some((_, socket)) => socket.next_inbound().await,
        None => std::future::pending().await,
    }
}

async fn sleep_until(clock: &dyn RuntimeClock, deadline: Option<ClockInstant>) {
    match deadline {
        Some(deadline) => clock.sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

async fn pending_for(
    store: &Mutex<dyn OutboxStorage>,
    caller: &Caller,
) -> Result<Vec<PendingWrite>, OutboxStoreError> {
    let mut store = store.lock().await;
    store.pending_for(caller).await
}

#[path = "driver_flush.rs"]
mod driver_flush;
#[path = "driver_tokens.rs"]
mod driver_tokens;

#[cfg(test)]
#[path = "store_faults_test.rs"]
mod store_faults_tests;

#[cfg(test)]
#[path = "driver_task_test.rs"]
mod tests;

#[cfg(test)]
#[path = "pre_admission_test.rs"]
mod pre_admission_tests;
