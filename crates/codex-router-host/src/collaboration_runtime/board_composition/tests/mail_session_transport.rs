//! Test-only transport delegates real I/O and adds only SDK-generated notice frames.
use super::*;

#[derive(Clone)]
pub(super) struct NoticeTransport {
    inner: ReqwestSessionTransport,
    ledger: Arc<NoticeLedger>,
    sender: mpsc::Sender<Bytes>,
    receiver: Arc<Mutex<mpsc::Receiver<Bytes>>>,
    initial_notice: Arc<AtomicBool>,
    pub observer: Arc<WireObserver>,
}
pub(super) struct NoticeConnection {
    inner: board_fleet_sync::ReqwestSessionConnection,
    ledger: Arc<NoticeLedger>,
    sender: mpsc::Sender<Bytes>,
    receiver: Arc<Mutex<mpsc::Receiver<Bytes>>>,
    initial_notice: Arc<AtomicBool>,
    observer: Arc<WireObserver>,
    held: Option<HeldFetchReply>,
}
impl NoticeTransport {
    pub fn new(
        inner: ReqwestSessionTransport,
        ledger: Arc<NoticeLedger>,
        initial_notice: bool,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(16);
        Self {
            inner,
            ledger,
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            initial_notice: Arc::new(AtomicBool::new(initial_notice)),
            observer: Arc::new(WireObserver::default()),
        }
    }
    pub async fn publish(&self, position: u64, stream: DeliveryStream) -> Result<(), TestError> {
        for frame in self.ledger.publish(position, stream).await? {
            self.sender.send(frame).await?;
        }
        Ok(())
    }
    pub async fn replay(&self, frames: Vec<Bytes>) -> Result<(), TestError> {
        for frame in frames {
            self.sender.send(frame).await?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) enum WireEvent {
    Renewal(fleet_sync_protocol::Epoch),
    Notice(u64),
    MailSubmit(fleet_sync_protocol::ItemId),
    SubmissionEpoch {
        item: fleet_sync_protocol::ItemId,
        epoch: Option<u64>,
    },
    GenericSubmit(fleet_sync_protocol::ItemId),
    Fetch {
        stream: DeliveryStream,
        max_bytes: u64,
        inbox_room_bytes: Option<u64>,
    },
    Fetched {
        stream: DeliveryStream,
        items: Vec<fleet_sync_protocol::ItemId>,
        entry_bytes: Vec<usize>,
    },
    Received(DeliveryStream),
    Report(fleet_sync_protocol::ItemId),
    StatusFetch {
        after: u64,
        limit: u32,
    },
    StatusFetched {
        through: u64,
        events: usize,
    },
    StatusAck {
        through: u64,
    },
    StatusAcked {
        floor: u64,
    },
    LostStatusAck {
        through: u64,
    },
    Publication {
        frame: fleet_sync_protocol::FrameId,
        item: fleet_sync_protocol::ItemId,
        status: fleet_sync_protocol::DeliveryStatus<collaboration_mail_protocol::CarrierAcceptance>,
    },
    Published {
        frame: fleet_sync_protocol::FrameId,
        applied: bool,
    },
}
pub(super) struct WireObserver {
    events: std::sync::Mutex<Vec<WireEvent>>,
    changed: watch::Sender<()>,
    next_hold: std::sync::Mutex<Option<FetchReplyPlan>>,
    lose_status_ack: AtomicBool,
}
impl Default for WireObserver {
    fn default() -> Self {
        Self {
            events: Default::default(),
            changed: watch::channel(()).0,
            next_hold: Default::default(),
            lose_status_ack: AtomicBool::new(false),
        }
    }
}
impl WireObserver {
    pub fn lose_next_status_ack(&self) {
        assert!(!self.lose_status_ack.swap(true, Ordering::SeqCst));
    }

    fn lose_ack_before_send(&self, bytes: &[u8]) -> bool {
        if let Ok(fleet_sync_protocol::ClientFrame::Request {
            body:
                OperationRequest::Delivery(fleet_sync_protocol::DeliveryRequest::StatusAck {
                    through,
                    ..
                }),
            ..
        }) =
            serde_json::from_slice::<fleet_sync_protocol::ClientFrame<BoardConsumerProtocol>>(bytes)
            && self.lose_status_ack.swap(false, Ordering::SeqCst)
        {
            self.record(WireEvent::LostStatusAck {
                through: through.value(),
            });
            return true;
        }
        false
    }

    pub fn hold_next_fetch(
        &self,
        stream: DeliveryStream,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.hold_reply(ReplyKind::Receive(stream))
    }
    pub fn hold_next_status_fetch(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.hold_reply(ReplyKind::Status)
    }
    pub fn hold_next_renewal(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.hold_reply(ReplyKind::Renewal)
    }
    pub fn hold_next_mail_submit(
        &self,
        item: fleet_sync_protocol::ItemId,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        self.hold_reply(ReplyKind::MailSubmit(item))
    }
    fn hold_reply(
        &self,
        kind: ReplyKind,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let previous = self.next_hold.lock().unwrap().replace(FetchReplyPlan {
            kind,
            entered,
            released,
        });
        assert!(previous.is_none(), "one next-Fetch reply gate at a time");
        (observed, release)
    }
    fn take_hold(&self, bytes: &[u8]) -> Option<HeldFetchReply> {
        let frame =
            serde_json::from_slice::<fleet_sync_protocol::ClientFrame<BoardConsumerProtocol>>(
                bytes,
            )
            .ok()?;
        let fleet_sync_protocol::ClientFrame::Request { frame, body } = frame else {
            return None;
        };
        let kind = match body {
            OperationRequest::Delivery(fleet_sync_protocol::DeliveryRequest::Fetch {
                stream,
                ..
            }) => ReplyKind::Receive(stream),
            OperationRequest::Delivery(fleet_sync_protocol::DeliveryRequest::StatusFetch {
                ..
            }) => ReplyKind::Status,
            OperationRequest::Lease(fleet_sync_protocol::LeaseRequest::Renew(_)) => {
                ReplyKind::Renewal
            }
            OperationRequest::Consumer(board_sync_protocol::BoardConsumerRequest::MailSubmit {
                item,
                ..
            }) => ReplyKind::MailSubmit(item.id()),
            _ => return None,
        };
        let mut planned = self.next_hold.lock().unwrap();
        if planned.as_ref().is_some_and(|plan| plan.kind == kind) {
            let plan = planned.take()?;
            Some(HeldFetchReply {
                frame,
                entered: Some(plan.entered),
                released: plan.released,
                bytes: None,
            })
        } else {
            None
        }
    }
    fn record(&self, event: WireEvent) {
        self.events.lock().unwrap().push(event);
        self.changed.send_replace(());
    }
    pub fn events(&self) -> Vec<WireEvent> {
        self.events.lock().unwrap().clone()
    }
    pub async fn wait_for(
        &self,
        ready: impl Fn(&[WireEvent]) -> bool,
        failure: &'static str,
    ) -> Result<Vec<WireEvent>, TestError> {
        let mut changed = self.changed.subscribe();
        tokio::time::timeout(BOUND, async {
            loop {
                let events = self.events();
                if ready(&events) {
                    return Ok(events);
                }
                changed.changed().await.map_err(io::Error::other)?;
            }
        })
        .await
        .map_err(|_| io::Error::other(failure))?
    }
    fn sent(&self, bytes: &[u8]) {
        use fleet_sync_protocol::{
            ClientAnswer, ClientFrame, DeliveryRequest, ServerCallAnswer, StatusPublicationResult,
        };
        if let Ok(frame) = serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(bytes) {
            let event = match frame {
                ClientFrame::Request {
                    body: OperationRequest::Lease(fleet_sync_protocol::LeaseRequest::Renew(request)),
                    ..
                } => Some(WireEvent::Renewal(request.epoch)),
                ClientFrame::Request {
                    body:
                        OperationRequest::Consumer(
                            board_sync_protocol::BoardConsumerRequest::MailSubmit {
                                item,
                                origin_epoch,
                            },
                        ),
                    ..
                } => {
                    self.record(WireEvent::SubmissionEpoch {
                        item: item.id(),
                        epoch: origin_epoch.map(|epoch| epoch.value()),
                    });
                    Some(WireEvent::MailSubmit(item.id()))
                }
                ClientFrame::Request {
                    body: OperationRequest::Delivery(request),
                    ..
                } => match request {
                    DeliveryRequest::Submit { item, .. } => {
                        Some(WireEvent::GenericSubmit(item.id()))
                    }
                    DeliveryRequest::Fetch {
                        stream,
                        max_bytes,
                        inbox_room_bytes,
                        ..
                    } => Some(WireEvent::Fetch {
                        stream,
                        max_bytes,
                        inbox_room_bytes,
                    }),
                    DeliveryRequest::Received { stream, .. } => Some(WireEvent::Received(stream)),
                    DeliveryRequest::Report { report, .. } => Some(WireEvent::Report(report.item)),
                    DeliveryRequest::StatusFetch { after, limit, .. } => {
                        Some(WireEvent::StatusFetch {
                            after: after.value(),
                            limit,
                        })
                    }
                    DeliveryRequest::StatusAck { through, .. } => Some(WireEvent::StatusAck {
                        through: through.value(),
                    }),
                    _ => None,
                },
                ClientFrame::Answer {
                    frame,
                    body: ClientAnswer::Crate(ServerCallAnswer::Published(result)),
                } => Some(WireEvent::Published {
                    frame,
                    applied: matches!(result, StatusPublicationResult::Applied),
                }),
                _ => None,
            };
            if let Some(event) = event {
                self.record(event);
            }
        }
    }
    fn received(&self, bytes: &[u8]) {
        use fleet_sync_protocol::{
            DeliveryAnswer, DeliveryEnvelope, FetchEntry, ServerCall, ServerFrame, ServerRequest,
        };
        if let Ok(frame) = serde_json::from_slice::<ServerFrame<BoardConsumerProtocol>>(bytes) {
            match frame {
                ServerFrame::Notice { position, .. } => {
                    self.record(WireEvent::Notice(position.value()))
                }
                ServerFrame::Answer {
                    body: OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(batch)),
                    ..
                } => self.record(WireEvent::StatusFetched {
                    through: batch.through.value(),
                    events: batch.events.len(),
                }),
                ServerFrame::Answer {
                    body:
                        OperationAnswer::Delivery(DeliveryAnswer::StatusAck(
                            fleet_sync_protocol::StatusAckResult::Acknowledged { floor },
                        )),
                    ..
                } => self.record(WireEvent::StatusAcked {
                    floor: floor.value(),
                }),
                ServerFrame::Answer {
                    body: OperationAnswer::Delivery(DeliveryAnswer::Fetch(result)),
                    ..
                } => {
                    let entry_bytes = result
                        .entries
                        .iter()
                        .map(|entry| serde_json::to_vec(entry).unwrap().len())
                        .collect();
                    let items = result
                        .entries
                        .iter()
                        .filter_map(|entry| match entry {
                            FetchEntry::Item {
                                item: DeliveryEnvelope::Item(item),
                                ..
                            } => Some(item.id()),
                            _ => None,
                        })
                        .collect();
                    self.record(WireEvent::Fetched {
                        stream: result.stream,
                        items,
                        entry_bytes,
                    });
                }
                ServerFrame::Request {
                    frame,
                    body: ServerRequest::Crate(ServerCall::PublishStatus { publication, .. }),
                } => self.record(WireEvent::Publication {
                    frame,
                    item: publication.item,
                    status: publication.status,
                }),
                _ => {}
            }
        }
    }
}
impl SessionTransport for NoticeTransport {
    type Connection = NoticeConnection;
    async fn connect(
        &self,
        endpoint: &fleet_sync_session::Endpoint,
        credential: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        Ok(NoticeConnection {
            inner: self.inner.connect(endpoint, credential).await?,
            ledger: self.ledger.clone(),
            sender: self.sender.clone(),
            receiver: self.receiver.clone(),
            initial_notice: self.initial_notice.clone(),
            observer: self.observer.clone(),
            held: None,
        })
    }
}
impl TransportConnection for NoticeConnection {
    async fn send(&mut self, frame: Bytes) -> Result<(), ConnectionClosed> {
        if self.observer.lose_ack_before_send(&frame) {
            return Err(ConnectionClosed::Transport);
        }
        if matches!(
            serde_json::from_slice::<fleet_sync_protocol::ClientFrame<BoardConsumerProtocol>>(
                &frame
            ),
            Ok(fleet_sync_protocol::ClientFrame::Hello { .. })
        ) {
            let mut frames = self
                .ledger
                .attach(&frame)
                .await
                .map_err(|_| ConnectionClosed::Transport)?;
            if self.initial_notice.swap(false, Ordering::SeqCst) {
                frames.extend(
                    self.ledger
                        .publish(1, DeliveryStream::Items)
                        .await
                        .map_err(|_| ConnectionClosed::Transport)?,
                );
            }
            for frame in frames {
                self.sender
                    .send(frame)
                    .await
                    .map_err(|_| ConnectionClosed::Transport)?;
            }
        }
        self.inner.send(frame.clone()).await?;
        self.observer.sent(&frame);
        if let Some(held) = self.observer.take_hold(&frame) {
            if self.held.is_some() {
                return Err(ConnectionClosed::Transport);
            }
            self.held = Some(held);
        }
        Ok(())
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        let mut incoming = self.receiver.lock().await;
        loop {
            let (received, released) =
                if let Some(held) = self.held.as_mut().filter(|held| held.bytes.is_some()) {
                    tokio::select! {
                        biased;
                        frame = incoming.recv() => (frame.map(Ok), false),
                        release = &mut held.released => {
                            if release.is_err() { return Some(Err(ConnectionClosed::Transport)); }
                            (held.bytes.take().map(Ok), true)
                        },
                        frame = self.inner.receive() => (frame, false),
                    }
                } else {
                    tokio::select! {
                        // SDK-created hints remain receivable while one real relay
                        // answer is held, so the bad interleaving is deterministic.
                        biased;
                        frame = incoming.recv() => (frame.map(Ok), false),
                        frame = self.inner.receive() => (frame, false),
                    }
                };
            if released {
                self.held = None;
            }
            if let Some(Ok(bytes)) = &received {
                if let Some(held) = self.held.as_mut()
                    && held.bytes.is_none()
                    && matches!(serde_json::from_slice::<fleet_sync_protocol::ServerFrame<BoardConsumerProtocol>>(bytes),
                        Ok(fleet_sync_protocol::ServerFrame::Answer { frame, .. }) if frame == held.frame)
                {
                    held.bytes = Some(bytes.clone());
                    if let Some(entered) = held.entered.take() {
                        let _ = entered.send(());
                    }
                    continue;
                }
                self.observer.received(bytes);
            }
            return received;
        }
    }
    async fn close(self, reason: fleet_sync_protocol::CloseReason) {
        self.inner.close(reason).await;
    }
}

struct FetchReplyPlan {
    kind: ReplyKind,
    entered: tokio::sync::oneshot::Sender<()>,
    released: tokio::sync::oneshot::Receiver<()>,
}
#[derive(Clone, Copy, Eq, PartialEq)]
enum ReplyKind {
    Receive(DeliveryStream),
    Status,
    Renewal,
    MailSubmit(fleet_sync_protocol::ItemId),
}
struct HeldFetchReply {
    frame: fleet_sync_protocol::FrameId,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    released: tokio::sync::oneshot::Receiver<()>,
    bytes: Option<Bytes>,
}
