//! The production driver against a scripted central over an in-memory socket,
//! on the injected ManualClock. It proves the driver's own behaviour (minute
//! flush, batch frame, outcomes, link loss, restart); a real central socket is
//! 2d's Turmoil journey.
use super::super::*;
use crate::outbox_store::{MacOutbox, NewOutboxItem};
use central_link_client::{
    FlushOutcomes, LinkCalls, PendingWriteState, RequestStatus, Submission, SubmitRefusal,
};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, MachineTokenText, ReplyPayload, decode_text, encode_text,
};
use router_clock::ManualClock;
use serde_json::json;
use std::sync::Mutex as StdMutex;
use std::time::Duration;
use tokio::sync::mpsc;
use workspace_model::{WorkspaceReply, WorkspaceResult};

const MARK: i64 = 1_763_380_800_000;
const INSTALLATION: &str = "019f0000-0000-7000-8000-0000000ea000";

fn uuid_v7(seed: u16) -> String {
    let mut random = [0_u8; 10];
    random[..2].copy_from_slice(&seed.to_be_bytes());
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + u64::from(seed), &random)
        .into_uuid()
        .to_string()
}

fn caller() -> Caller {
    serde_json::from_value(json!({"kind": "session", "value": {
        "machineId": INSTALLATION, "endpoint": "claude-local", "sessionId": "s1"}}))
    .unwrap()
}

fn thread_post(request: &str) -> RequestPayload {
    serde_json::from_value(json!({"kind": "workspace", "request": {
        "requestId": request, "workspaceId": uuid_v7(9000),
        "operation": {"kind": "threadPost", "threadId": uuid_v7(9001), "body": "hi", "addressees": []}}}))
    .unwrap()
}

fn list_areas() -> RequestPayload {
    serde_json::from_value(json!({"kind": "workspace", "request": {
        "requestId": null, "workspaceId": uuid_v7(9000),
        "operation": {"kind": "list", "query": {"kind": "areas"}, "after": 0, "limit": 5}}}))
    .unwrap()
}

fn reply() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Conflict,
        actors: Vec::new(),
    })
}

/// The test's end of one socket: frames from the driver, and a sender to it.
struct CentralEnd {
    from_router: mpsc::UnboundedReceiver<String>,
    to_router: mpsc::UnboundedSender<String>,
}

impl CentralEnd {
    /// The next data frame; heartbeat pings are answered and skipped.
    async fn frame(&mut self) -> LinkFrame {
        loop {
            let text = self.from_router.recv().await.expect("router frame");
            match decode_text(&text, FrameDirection::ToCentral).unwrap() {
                LinkFrame::Ping => self.send(LinkFrame::Pong),
                frame => return frame,
            }
        }
    }
    fn send(&self, frame: LinkFrame) {
        let _ = self
            .to_router
            .send(encode_text(&frame, FrameDirection::ToRouter).unwrap());
    }
    fn welcome(&self) {
        self.send(LinkFrame::Data(DataFrame::Welcome {
            machine_id: INSTALLATION.parse().unwrap(),
            central_time: chrono::DateTime::from_timestamp_millis(MARK).unwrap(),
        }));
    }
}

struct ChannelSocket {
    to_central: mpsc::UnboundedSender<String>,
    from_central: mpsc::UnboundedReceiver<String>,
}

impl LinkSocket for ChannelSocket {
    fn send_text(&mut self, text: String) -> TransportFuture<'_, Result<(), LinkTransportError>> {
        let sent = self
            .to_central
            .send(text)
            .map_err(|_| LinkTransportError::Send);
        Box::pin(async move { sent })
    }
    fn next_text(&mut self) -> TransportFuture<'_, Option<String>> {
        Box::pin(self.from_central.recv())
    }
    fn close(&mut self) -> TransportFuture<'_, ()> {
        self.from_central.close();
        Box::pin(async {})
    }
}

/// Each connect hands the test a fresh central end.
struct ChannelTransport {
    connections: mpsc::UnboundedSender<CentralEnd>,
}

impl LinkTransport for ChannelTransport {
    fn connect<'call>(
        &'call self,
        _token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        let (to_central, from_router) = mpsc::unbounded_channel();
        let (to_router, from_central) = mpsc::unbounded_channel();
        let _ = self.connections.send(CentralEnd {
            from_router,
            to_router,
        });
        Box::pin(async move {
            Ok(Box::new(ChannelSocket {
                to_central,
                from_central,
            }) as Box<dyn LinkSocket>)
        })
    }
}

struct FixedToken;
impl MachineTokenSource for FixedToken {
    fn current(&self) -> TransportFuture<'_, Option<MachineTokenText>> {
        Box::pin(async { Some("fixture.machine.token".to_owned().try_into().unwrap()) })
    }
}

struct NoJitter;
impl FlushJitter for NoJitter {
    fn jitter_millis(&self, _mark: SyncTime) -> i64 {
        0
    }
}

#[derive(Default)]
struct RecordedOutcomes(StdMutex<Vec<FlushOutcomes>>);
impl FlushOutcomeSink for RecordedOutcomes {
    fn deliver(&self, outcomes: FlushOutcomes) {
        self.0.lock().unwrap().push(outcomes);
    }
}

struct Harness {
    clock: Arc<ManualClock>,
    calls: RouterLinkCalls,
    connections: mpsc::UnboundedReceiver<CentralEnd>,
    outcomes: Arc<RecordedOutcomes>,
    store: Arc<Mutex<ProviderOperationStore>>,
    stop: CancellationToken,
    tasks: TaskTracker,
    _root: tempfile::TempDir,
}

impl Harness {
    async fn start(root: tempfile::TempDir, clock: Arc<ManualClock>) -> Self {
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
                .await
                .unwrap(),
        ));
        Self::start_on(root, clock, store)
    }

    fn start_on(
        root: tempfile::TempDir,
        clock: Arc<ManualClock>,
        store: Arc<Mutex<ProviderOperationStore>>,
    ) -> Self {
        let (sender, connections) = mpsc::unbounded_channel();
        let outcomes = Arc::new(RecordedOutcomes::default());
        let stop = CancellationToken::new();
        let tasks = TaskTracker::new();
        let calls = RouterLinkCalls::start(
            CentralLinkParts {
                installation: INSTALLATION.parse().unwrap(),
                clock: clock.clone(),
                store: store.clone(),
                transport: Arc::new(ChannelTransport {
                    connections: sender,
                }),
                tokens: Arc::new(FixedToken),
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
            store,
            stop,
            tasks,
            _root: root,
        }
    }

    async fn connected(&mut self) -> CentralEnd {
        let central = self.connections.recv().await.expect("a connection");
        central.welcome();
        // A status read is a round trip through the driver's loop.
        let _ = self
            .calls
            .request_status(caller(), uuid_v7(1).parse().unwrap())
            .await;
        central
    }

    async fn stop(self) {
        self.stop.cancel();
        self.tasks.close();
        self.tasks.wait().await;
    }
}

fn clock_at(millis: i64) -> Arc<ManualClock> {
    Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(millis)).unwrap())
}

#[tokio::test]
async fn a_post_flushes_on_its_mark_as_one_batch_and_its_answer_is_reported() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let id = uuid_v7(1);

    let submission = harness.calls.submit(caller(), thread_post(&id)).await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("the flush sends one batch frame");
    };
    let [member] = requests.requests() else {
        panic!("one member");
    };
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: member.correlation_id().clone(),
        result: Box::new(reply()),
    }));
    let status = loop {
        let status = harness
            .calls
            .request_status(caller(), id.parse().unwrap())
            .await;
        if status != RequestStatus::Sent {
            break status;
        }
        tokio::task::yield_now().await;
    };

    assert!(matches!(submission, Submission::InOutbox(ref receipt)
        if receipt.flush_at == SyncTime::from_unix_millis(MARK)));
    assert_eq!(status, RequestStatus::Answered(reply()));
    let outcomes = harness.outcomes.0.lock().unwrap().clone();
    assert_eq!(outcomes.len(), 1);
    assert!(outcomes[0].complete);
    assert_eq!(outcomes[0].outcomes[0].request_id.as_str(), id);
    harness.stop().await;
}

#[tokio::test]
async fn a_lost_link_returns_the_batch_as_sent_answer_lost_and_resends_the_same_ids() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let central = harness.connected().await;
    let id = uuid_v7(2);
    harness.calls.submit(caller(), thread_post(&id)).await;
    let mut central = central;

    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("first batch");
    };
    let first = requests.requests()[0].clone();
    drop(central);
    let pending = harness.calls.pending_for(caller()).await.unwrap();
    // The client reconnects after its 1 s backoff on the injected clock.
    harness.clock.advance(Duration::from_millis(1_000)).unwrap();
    let mut central = harness.connected().await;
    harness
        .clock
        .advance(Duration::from_millis(60_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("resent batch");
    };

    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].state, PendingWriteState::SentAnswerLost);
    let outcomes = harness.outcomes.0.lock().unwrap().clone();
    assert!(matches!(outcomes.as_slice(), [report] if !report.complete));
    let resent = &requests.requests()[0];
    assert_eq!(
        resent.payload().write_request_id(),
        first.payload().write_request_id()
    );
    assert_ne!(resent.correlation_id(), first.correlation_id());
    harness.stop().await;
}

#[tokio::test]
async fn immediate_calls_are_answered_on_the_link_and_refused_when_it_is_down() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let down = harness.calls.submit(caller(), list_areas()).await;
    let mut central = harness.connected().await;

    let answered = tokio::spawn({
        let calls = harness.calls.clone();
        async move { calls.submit(caller(), list_areas()).await }
    });
    let LinkFrame::Data(DataFrame::Request { request }) = central.frame().await else {
        panic!("an immediate request frame");
    };
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: request.correlation_id().clone(),
        result: Box::new(reply()),
    }));

    assert!(matches!(
        down,
        Submission::Refused(SubmitRefusal::CentralUnavailable)
    ));
    assert!(matches!(answered.await.unwrap(), Submission::Answered(_)));
    harness.stop().await;
}

#[tokio::test]
async fn a_restart_resends_writes_that_were_sent_without_an_answer() {
    let root = tempfile::tempdir().unwrap();
    let clock = clock_at(MARK - 20_000);
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("provider-operations.sqlite"))
            .await
            .unwrap(),
    ));
    let id = uuid_v7(3);
    {
        let mut locked = store.lock().await;
        let mut outbox = MacOutbox::new(&mut locked);
        let item = NewOutboxItem {
            request: id.parse().unwrap(),
            caller: caller(),
            payload: thread_post(&id),
            flush_at: SyncTime::from_unix_millis(MARK),
        };
        outbox
            .insert(&item, SyncTime::from_unix_millis(MARK - 30_000))
            .await
            .unwrap();
        outbox
            .mark_sent(
                std::slice::from_ref(&item.request),
                SyncTime::from_unix_millis(MARK - 25_000),
            )
            .await
            .unwrap();
    }

    let mut harness = Harness::start_on(root, clock, store);
    let mut central = harness.connected().await;
    harness
        .clock
        .advance(Duration::from_millis(20_000))
        .unwrap();
    let LinkFrame::Data(DataFrame::Batch { requests }) = central.frame().await else {
        panic!("the restored write is resent on its mark");
    };

    assert_eq!(
        requests.requests()[0]
            .payload()
            .write_request_id()
            .map(|id| id.as_str().to_owned()),
        Some(id)
    );
    let _ = &harness.store;
    harness.stop().await;
}
