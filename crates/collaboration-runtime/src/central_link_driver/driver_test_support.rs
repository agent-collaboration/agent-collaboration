//! Test harness for the link driver: a scripted central over an in-memory
//! socket, a scripted token source, no jitter and a recording outcome sink.
use super::*;
use crate::{MachineLocalOutbox, MachineLocalStore, MachineLocalStoreError};

/// machine_local.sqlite opened as the driver's outbox.
pub(crate) async fn open_outbox(
    path: &std::path::Path,
) -> Result<MachineLocalOutbox, MachineLocalStoreError> {
    Ok(MachineLocalOutbox::new(
        MachineLocalStore::open(path).await?,
    ))
}
use central_link_client::{FlushOutcomes, LinkCalls};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, MachineTokenText, ReplyPayload, TokenRenewalResult,
    decode_text, encode_text,
};
use router_clock::ManualClock;
use serde_json::json;
use std::sync::Mutex as StdMutex;
use tokio::sync::{mpsc, watch};
use workspace_model::{WorkspaceReply, WorkspaceResult};

pub(crate) const MARK: i64 = 1_763_380_800_000;
pub(crate) const INSTALLATION: &str = "019f0000-0000-7000-8000-0000000ea000";

pub(crate) fn uuid_v7(seed: u16) -> String {
    let mut random = [0_u8; 10];
    random[..2].copy_from_slice(&seed.to_be_bytes());
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + u64::from(seed), &random)
        .into_uuid()
        .to_string()
}

pub(crate) fn caller() -> Caller {
    serde_json::from_value(json!({"kind": "session", "value": {
        "machineId": INSTALLATION, "endpoint": "claude-local", "sessionId": "s1"}}))
    .unwrap()
}

pub(crate) fn thread_post(request: &str) -> RequestPayload {
    serde_json::from_value(json!({"kind": "workspace", "request": {
        "requestId": request, "workspaceId": uuid_v7(9000),
        "operation": {"kind": "threadPost", "threadId": uuid_v7(9001), "body": "hi", "addressees": []}}}))
    .unwrap()
}

pub(crate) fn list_areas() -> RequestPayload {
    serde_json::from_value(json!({"kind": "workspace", "request": {
        "requestId": null, "workspaceId": uuid_v7(9000),
        "operation": {"kind": "list", "query": {"kind": "areas", "includeArchived": false}, "cursor": {"order": "oldestFirst", "position": null}, "limit": 5}}}))
    .unwrap()
}

/// Central's answer when link admission refuses a write it did not apply.
pub(crate) fn machine_revoked() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Refused(workspace_model::RefusalReason::MachineRevoked),
        actors: Vec::new(),
    })
}

pub(crate) fn reply() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Conflict,
        actors: Vec::new(),
    })
}

/// The test's end of one socket: frames from the driver, and a sender to it.
pub(crate) struct CentralEnd {
    pub(crate) from_router: mpsc::UnboundedReceiver<String>,
    pub(crate) to_router: mpsc::UnboundedSender<LinkInbound>,
    /// The token this connection was opened with.
    pub(crate) token: MachineTokenText,
}

impl CentralEnd {
    /// The next data frame; heartbeat pings are answered and skipped.
    pub(crate) async fn frame(&mut self) -> LinkFrame {
        loop {
            let text = self.from_router.recv().await.expect("router frame");
            match decode_text(&text, FrameDirection::ToCentral).unwrap() {
                LinkFrame::Ping => self.send(LinkFrame::Pong),
                frame => return frame,
            }
        }
    }
    pub(crate) fn send(&self, frame: LinkFrame) {
        let _ = self.to_router.send(LinkInbound::Text(
            encode_text(&frame, FrameDirection::ToRouter).unwrap(),
        ));
    }
    /// Central closes the link as authority lapsed (1008).
    pub(crate) fn close_lapsed(&self) {
        let _ = self
            .to_router
            .send(LinkInbound::Closed(LinkClosed::AuthorityLapsed));
    }
    pub(crate) fn welcome(&self) {
        self.send(LinkFrame::Data(DataFrame::Welcome {
            machine_id: INSTALLATION.parse().unwrap(),
            central_time: chrono::DateTime::from_timestamp_millis(MARK).unwrap(),
        }));
    }
}

pub(crate) struct ChannelSocket {
    to_central: mpsc::UnboundedSender<String>,
    from_central: mpsc::UnboundedReceiver<LinkInbound>,
}

impl LinkSocket for ChannelSocket {
    fn send_text(&mut self, text: String) -> TransportFuture<'_, Result<(), LinkTransportError>> {
        let sent = self
            .to_central
            .send(text)
            .map_err(|_| LinkTransportError::Send);
        Box::pin(async move { sent })
    }
    fn next_inbound(&mut self) -> TransportFuture<'_, LinkInbound> {
        Box::pin(async move {
            self.from_central
                .recv()
                .await
                .unwrap_or(LinkInbound::Closed(LinkClosed::Other))
        })
    }
    fn close(&mut self) -> TransportFuture<'_, ()> {
        self.from_central.close();
        Box::pin(async {})
    }
}

/// Each connect hands the test a fresh central end.
pub(crate) struct ChannelTransport {
    pub(crate) connections: mpsc::UnboundedSender<CentralEnd>,
}

impl LinkTransport for ChannelTransport {
    fn connect<'call>(
        &'call self,
        token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        let (to_central, from_router) = mpsc::unbounded_channel();
        let (to_router, from_central) = mpsc::unbounded_channel();
        let _ = self.connections.send(CentralEnd {
            from_router,
            to_router,
            token: token.clone(),
        });
        Box::pin(async move {
            Ok(Box::new(ChannelSocket {
                to_central,
                from_central,
            }) as Box<dyn LinkSocket>)
        })
    }
}

pub(crate) fn fixture_token(text: &str) -> MachineTokenText {
    text.to_owned().try_into().unwrap()
}

/// A fake B3 source: the test sets its availability and reads back every
/// renewal result the driver reported.
pub(crate) struct ScriptedTokens {
    availability: StdMutex<TokenAvailability>,
    version: watch::Sender<u64>,
    seen: Mutex<watch::Receiver<u64>>,
    pub(crate) renewals: StdMutex<Vec<TokenRenewalResult>>,
    /// How often the driver read the availability: once per connect attempt.
    pub(crate) reads: std::sync::atomic::AtomicUsize,
}

impl ScriptedTokens {
    pub(crate) fn new(availability: TokenAvailability) -> Self {
        let (version, seen) = watch::channel(0);
        Self {
            availability: StdMutex::new(availability),
            version,
            seen: Mutex::new(seen),
            renewals: StdMutex::new(Vec::new()),
            reads: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(crate) fn set(&self, availability: TokenAvailability) {
        *self.availability.lock().unwrap() = availability;
        self.version.send_modify(|version| *version += 1);
    }
}

impl MachineTokenSource for ScriptedTokens {
    fn availability(&self) -> TokenAvailability {
        self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.availability.lock().unwrap().clone()
    }
    fn changed(&self) -> TransportFuture<'_, ()> {
        Box::pin(async move {
            if self.seen.lock().await.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        })
    }
    fn report_renewal(&self, outcome: &TokenRenewalResult) {
        self.renewals.lock().unwrap().push(outcome.clone());
    }
}

pub(crate) struct NoJitter;
impl FlushJitter for NoJitter {
    fn jitter_millis(&self, _mark: SyncTime) -> i64 {
        0
    }
}

#[derive(Default)]
pub(crate) struct RecordedOutcomes(pub(crate) StdMutex<Vec<FlushOutcomes>>);
impl FlushOutcomeSink for RecordedOutcomes {
    fn deliver(&self, outcomes: FlushOutcomes) {
        self.0.lock().unwrap().push(outcomes);
    }
}

pub(crate) struct Harness {
    pub(crate) clock: Arc<ManualClock>,
    pub(crate) calls: RouterLinkCalls,
    pub(crate) connections: mpsc::UnboundedReceiver<CentralEnd>,
    pub(crate) outcomes: Arc<RecordedOutcomes>,
    pub(crate) tokens: Arc<ScriptedTokens>,
    pub(crate) store: Arc<Mutex<MachineLocalOutbox>>,
    pub(crate) stop: CancellationToken,
    pub(crate) tasks: TaskTracker,
    _root: tempfile::TempDir,
}

impl Harness {
    pub(crate) async fn start(root: tempfile::TempDir, clock: Arc<ManualClock>) -> Self {
        let store = Arc::new(Mutex::new(
            open_outbox(&root.path().join("machine_local.sqlite"))
                .await
                .unwrap(),
        ));
        Self::start_on(root, clock, store)
    }

    pub(crate) fn start_on(
        root: tempfile::TempDir,
        clock: Arc<ManualClock>,
        store: Arc<Mutex<MachineLocalOutbox>>,
    ) -> Self {
        let ready = TokenAvailability::Ready(fixture_token("fixture.machine.token"));
        Self::launch(root, clock, store, ready)
    }

    /// Starts on a fresh store with the token source in `availability`.
    pub(crate) async fn start_with_tokens(
        root: tempfile::TempDir,
        clock: Arc<ManualClock>,
        availability: TokenAvailability,
    ) -> Self {
        let store = Arc::new(Mutex::new(
            open_outbox(&root.path().join("machine_local.sqlite"))
                .await
                .unwrap(),
        ));
        Self::launch(root, clock, store, availability)
    }

    fn launch(
        root: tempfile::TempDir,
        clock: Arc<ManualClock>,
        store: Arc<Mutex<MachineLocalOutbox>>,
        availability: TokenAvailability,
    ) -> Self {
        let (sender, connections) = mpsc::unbounded_channel();
        let outcomes = Arc::new(RecordedOutcomes::default());
        let tokens = Arc::new(ScriptedTokens::new(availability));
        let stop = CancellationToken::new();
        let tasks = TaskTracker::new();
        let calls = RouterLinkCalls::start(
            CentralLinkParts {
                installation: INSTALLATION.parse().unwrap(),
                clock: clock.clone(),
                store: store.clone() as Arc<Mutex<dyn OutboxStorage>>,
                transport: Arc::new(ChannelTransport {
                    connections: sender,
                }),
                tokens: tokens.clone(),
                jitter: Arc::new(NoJitter),
                outcomes: outcomes.clone(),
            },
            &tasks,
            stop.clone(),
        );
        Self {
            clock,
            calls,
            connections,
            outcomes,
            tokens,
            store,
            stop,
            tasks,
            _root: root,
        }
    }

    pub(crate) async fn connected(&mut self) -> CentralEnd {
        let central = self.connections.recv().await.expect("a connection");
        central.welcome();
        // A status read is a round trip through the driver's loop.
        let _ = self
            .calls
            .request_status(caller(), uuid_v7(1).parse().unwrap())
            .await;
        central
    }

    pub(crate) async fn stop(self) {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

pub(crate) fn clock_at(millis: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(millis)).unwrap())
}
