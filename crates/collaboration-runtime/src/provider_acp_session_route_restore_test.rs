//! A pending restore and later requests on one real provider ACP route.

use super::*;
use codex_acp_adapter::{AcpOutputSender, QueuedAcpFrame};
use router_clock::ManualClock;
use std::{future::poll_fn, task::Poll, time::Duration};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RestoreMethod {
    Load,
    Resume,
}

impl RestoreMethod {
    fn method(self) -> &'static str {
        match self {
            Self::Load => "session/load",
            Self::Resume => "session/resume",
        }
    }
}

struct RestoreGate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}

#[derive(Clone, Copy)]
enum InventoryRead {
    Validation,
    Reply,
}

struct HeldInventoryRead {
    phase: InventoryRead,
    gate: RestoreGate,
}

pub(super) struct RestoreFixture {
    pub(super) other: SessionRef,
    pub(super) state: Mutex<SessionState>,
    pub(super) queue_reads: Mutex<Vec<SessionRef>>,
    calls: Mutex<Vec<(RestoreMethod, SessionRef)>>,
    clock: Arc<ManualClock>,
    gate: Mutex<Option<RestoreGate>>,
    inventory: Mutex<Option<HeldInventoryRead>>,
    attachment: Mutex<Option<RestoreGate>>,
    attachments: Mutex<usize>,
}

impl RestoreFixture {
    pub(super) fn inventory(
        &self,
        summaries: Vec<HubSessionSummary>,
    ) -> HubFuture<'_, Vec<HubSessionSummary>> {
        let unloaded = *self.state.lock().expect("restore state") == SessionState::Unloaded;
        let mut pending = self.inventory.lock().expect("inventory gate");
        let held =
            pending.take_if(|read| unloaded == matches!(read.phase, InventoryRead::Validation));
        Box::pin(async move {
            if let Some(held) = held {
                held.gate.entered.send(()).expect("inventory read reached");
                held.gate.release.await.expect("inventory read released");
            }
            Ok(summaries)
        })
    }

    fn hold_inventory(&self, phase: InventoryRead) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, reading) = oneshot::channel();
        let (release, released) = oneshot::channel();
        self.inventory
            .lock()
            .expect("inventory gate")
            .replace(HeldInventoryRead {
                phase,
                gate: RestoreGate {
                    entered,
                    release: released,
                },
            });
        (reading, release)
    }

    pub(super) fn attach(
        &self,
        attachment: SessionEventAttachment,
    ) -> HubFuture<'_, SessionEventAttachment> {
        *self.attachments.lock().expect("attachment count") += 1;
        let gate = self.attachment.lock().expect("attachment gate").take();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.entered.send(()).expect("attachment reached");
                gate.release.await.expect("attachment released");
            }
            Ok(attachment)
        })
    }

    fn hold_attachment(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, attaching) = oneshot::channel();
        let (release, released) = oneshot::channel();
        *self.attachment.lock().expect("attachment gate") = Some(RestoreGate {
            entered,
            release: released,
        });
        (attaching, release)
    }

    pub(super) fn restore(
        &self,
        method: RestoreMethod,
        command: SessionTargetCommand,
    ) -> CommandFuture<'_, ()> {
        self.calls
            .lock()
            .expect("restore calls")
            .push((method, command.target));
        let gate = self.gate.lock().expect("restore gate").take();
        Box::pin(async move {
            let gate = gate.expect("one restore reached the fixture");
            gate.entered.send(()).expect("restore handshake");
            router_clock::timeout(self.clock.as_ref(), Duration::from_secs(30), gate.release)
                .await
                .expect("fixture clock did not expire")
                .expect("restore released");
            *self.state.lock().expect("restore state") = SessionState::Idle;
            Ok(())
        })
    }
}

struct RestoreConnection {
    backend: Arc<RecordingSessionBackend>,
    entered: oneshot::Receiver<()>,
    release: oneshot::Sender<()>,
    input: AcpOutputSender,
    replies: mpsc::Receiver<QueuedAcpFrame>,
    closed: CancellationToken,
    route: AcpRouteFuture,
}

impl RestoreConnection {
    fn new() -> Self {
        let endpoint = serde_json::from_value(json!({
            "serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89", "endpointId":"claude-local"
        }))
        .expect("endpoint");
        let mut backend = RecordingSessionBackend::new(endpoint).expect("backend");
        let mut other = backend.session.clone();
        other.session_id = "other-session".to_owned().try_into().expect("session id");
        let (report_entered, entered) = oneshot::channel();
        let (release, wait_for_release) = oneshot::channel();
        let clock = Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH));
        Arc::get_mut(&mut backend)
            .expect("fixture owns the backend")
            .restore = Some(RestoreFixture {
            other,
            state: Mutex::new(SessionState::Unloaded),
            queue_reads: Mutex::default(),
            calls: Mutex::default(),
            clock: clock.clone(),
            gate: Mutex::new(Some(RestoreGate {
                entered: report_entered,
                release: wait_for_release,
            })),
            inventory: Mutex::default(),
            attachment: Mutex::default(),
            attachments: Mutex::default(),
        });
        let closed = CancellationToken::new();
        let (input, input_frames) = bounded_acp_output(closed.clone());
        let (output, replies) = bounded_acp_output(closed.clone());
        let route = Box::new(ProviderAcpSessionRoute::new(
            backend.session.endpoint.clone(),
            backend.clone(),
            backend.clone(),
            clock,
        ))
        .run(
            AcpRouterChannels {
                input: input_frames,
                output,
                closed: closed.clone(),
            },
            AcpConnectionContext {
                actor: Some(acp_actor().expect("actor")),
                client_profile: None,
                client_supports_elicitation_form: false,
            },
        );
        Self {
            backend,
            entered,
            release,
            input,
            replies,
            closed,
            route,
        }
    }

    async fn hold_restore(&mut self, method: &str) {
        request(&self.input, method, json!("first"), &self.backend.session).await;
        self.wait_for_restore().await;
    }

    async fn wait_for_restore(&mut self) {
        tokio::select! {
            entered = &mut self.entered => entered.expect("host restore handshake"),
            result = &mut self.route => panic!("route exited before entering the restore: {result:?}"),
        }
    }

    fn fixture(&self) -> &RestoreFixture {
        self.backend.restore.as_ref().expect("restore fixture")
    }
}

async fn request(input: &AcpOutputSender, method: &str, id: Value, session: &SessionRef) {
    let mut params = json!({"sessionId":session.session_id.as_str()});
    if method == "session/load" {
        params["cwd"] = json!("/");
        params["mcpServers"] = json!([]);
    }
    input
        .send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .await
        .expect("ACP request");
}

async fn next_response(
    route: &mut AcpRouteFuture,
    replies: &mut mpsc::Receiver<QueuedAcpFrame>,
) -> Value {
    loop {
        let frame = tokio::select! {
            frame = replies.recv() => frame.expect("ACP response"),
            result = &mut *route => panic!("route exited before the reply: {result:?}"),
        };
        if frame.get("id").is_some() {
            return (*frame).clone();
        }
    }
}

#[tokio::test]
async fn held_load_leaves_other_session_responsive_and_same_session_ordered() {
    check_restore_ordering(RestoreMethod::Load, RestoreMethod::Resume).await;
}

#[tokio::test]
async fn held_resume_leaves_other_session_responsive_and_same_session_ordered() {
    check_restore_ordering(RestoreMethod::Resume, RestoreMethod::Load).await;
}

async fn check_restore_ordering(restore: RestoreMethod, later_restore: RestoreMethod) {
    let mut connection = RestoreConnection::new();
    connection.hold_restore(restore.method()).await;
    let fixture = connection
        .backend
        .restore
        .as_ref()
        .expect("restore fixture");
    request(
        &connection.input,
        later_restore.method(),
        json!(2),
        &connection.backend.session,
    )
    .await;
    request(
        &connection.input,
        "_session/queue/list",
        json!(3),
        &connection.backend.session,
    )
    .await;
    request(
        &connection.input,
        "_session/queue/list",
        json!(4),
        &fixture.other,
    )
    .await;

    // The three inputs are ready. Drive the route until it first waits for an
    // external event. B's ready lookup can answer in that poll while A is held.
    poll_fn(|cx| {
        assert!(connection.route.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let other_reply = connection
        .replies
        .try_recv()
        .expect("B must reply before A's load is released");
    assert_eq!(
        *other_reply,
        json!({"jsonrpc":"2.0","id":4,"result":{"items":[]}})
    );
    assert_eq!(
        fixture.queue_reads.lock().expect("calls").as_slice(),
        std::slice::from_ref(&fixture.other)
    );

    connection.release.send(()).expect("release A's load");
    let first = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(first["id"], "first", "{first}");
    assert_eq!(
        first["result"]["sessionId"],
        connection.backend.session.session_id.as_str(),
        "{first}"
    );
    assert_eq!(
        first["result"]["_meta"]["sessionProfile"]["historyUnavailable"],
        restore == RestoreMethod::Resume,
        "{first}"
    );
    let duplicate = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(duplicate["id"], 2, "{duplicate}");
    assert_eq!(duplicate["error"]["code"], -32000, "{duplicate}");
    assert_eq!(
        duplicate["error"]["message"],
        "session was loading when this request arrived; check the first restore reply before retrying",
        "{duplicate}"
    );
    let queue = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(queue, json!({"jsonrpc":"2.0","id":3,"result":{"items":[]}}));
    assert_eq!(
        *fixture.calls.lock().expect("restore calls"),
        [(restore, connection.backend.session.clone())]
    );
    connection.closed.cancel();
    connection.route.await.expect("route shutdown");
}

#[tokio::test]
async fn closing_connection_drops_pending_resume_before_returning() {
    let mut connection = RestoreConnection::new();
    connection.hold_restore("session/resume").await;
    connection.closed.cancel();
    connection.route.await.expect("route shutdown");
    assert!(
        connection.release.is_closed(),
        "the route left the restore task running"
    );
}

#[tokio::test]
async fn held_restore_inventory_validation_leaves_the_other_session_responsive() {
    let mut connection = RestoreConnection::new();
    let other = connection.fixture().other.clone();
    let (reading, release) = connection
        .fixture()
        .hold_inventory(InventoryRead::Validation);
    request(
        &connection.input,
        "session/load",
        json!("first"),
        &connection.backend.session,
    )
    .await;
    reaches_inventory(&mut connection.route, reading).await;

    assert_other_session_replies(
        &connection.input,
        &mut connection.route,
        &mut connection.replies,
        &other,
    )
    .await;

    release.send(()).expect("release validation");
    connection.wait_for_restore().await;
    connection.release.send(()).expect("release activation");
    let reply = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(reply["id"], "first");
    assert_eq!(
        reply["result"]["sessionId"],
        connection.backend.session.session_id.as_str()
    );
    connection.closed.cancel();
    connection.route.await.expect("route shutdown");
}

#[tokio::test]
async fn held_restore_reply_inventory_leaves_the_other_session_responsive() {
    let mut connection = RestoreConnection::new();
    let other = connection.fixture().other.clone();
    let (reading, release) = connection.fixture().hold_inventory(InventoryRead::Reply);
    connection.hold_restore("session/resume").await;
    connection.release.send(()).expect("release activation");
    reaches_inventory(&mut connection.route, reading).await;
    request(
        &connection.input,
        "_session/queue/list",
        json!("same-session"),
        &connection.backend.session,
    )
    .await;

    assert_other_session_replies(
        &connection.input,
        &mut connection.route,
        &mut connection.replies,
        &other,
    )
    .await;

    release.send(()).expect("release reply inventory");
    let reply = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(reply["id"], "first");
    assert_eq!(
        reply["result"]["sessionId"],
        connection.backend.session.session_id.as_str()
    );
    let queued = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(
        queued,
        json!({"jsonrpc":"2.0","id":"same-session","result":{"items":[]}})
    );
    connection.closed.cancel();
    connection.route.await.expect("route shutdown");
}

#[tokio::test]
async fn held_restore_attachment_expires_without_blocking_other_sessions_or_reloading() {
    let mut connection = RestoreConnection::new();
    let backend = connection.backend.clone();
    let fixture = backend.restore.as_ref().expect("restore fixture");
    let clock = fixture.clock.clone();
    let other = fixture.other.clone();
    let (attaching, release_attachment) = fixture.hold_attachment();
    connection.hold_restore("session/load").await;
    connection.release.send(()).expect("provider activates A");
    reaches_inventory(&mut connection.route, attaching).await;
    let session = connection.backend.session.clone();
    request(
        &connection.input,
        "session/load",
        json!("duplicate"),
        &session,
    )
    .await;
    request(
        &connection.input,
        "_session/queue/list",
        json!("same-session"),
        &session,
    )
    .await;
    assert_other_session_replies(
        &connection.input,
        &mut connection.route,
        &mut connection.replies,
        &other,
    )
    .await;
    assert_eq!(
        fixture.queue_reads.lock().expect("calls").as_slice(),
        &[other]
    );

    clock
        .advance(Duration::from_secs(10))
        .expect("expire the post-activation deadline");
    for id in ["first", "duplicate"] {
        assert_eq!(
            next_response(&mut connection.route, &mut connection.replies).await,
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"the store didn't respond in time"}}),
        );
    }
    assert_eq!(
        next_response(&mut connection.route, &mut connection.replies).await,
        json!({"jsonrpc":"2.0","id":"same-session","result":{"items":[]}}),
    );
    assert!(
        release_attachment.is_closed(),
        "the first attachment was cancelled"
    );
    assert_eq!(*fixture.attachments.lock().expect("attachment count"), 1);
    assert_eq!(*fixture.state.lock().expect("state"), SessionState::Idle);

    request(&connection.input, "session/load", json!("fresh"), &session).await;
    let reply = next_response(&mut connection.route, &mut connection.replies).await;
    assert_eq!(reply["id"], "fresh");
    assert_eq!(reply["result"]["sessionId"], session.session_id.as_str());
    assert_eq!(*fixture.attachments.lock().expect("attachment count"), 2);
    assert_eq!(
        fixture.calls.lock().expect("restore calls").as_slice(),
        &[(RestoreMethod::Load, session)],
    );
    connection.closed.cancel();
    connection.route.await.expect("route shutdown");
}

async fn reaches_inventory(route: &mut AcpRouteFuture, reading: oneshot::Receiver<()>) {
    tokio::select! {
        entered = reading => entered.expect("inventory read handshake"),
        result = route => panic!("route exited before the inventory read: {result:?}"),
    }
}

async fn assert_other_session_replies(
    input: &AcpOutputSender,
    route: &mut AcpRouteFuture,
    replies: &mut mpsc::Receiver<QueuedAcpFrame>,
    other: &SessionRef,
) {
    request(input, "_session/queue/list", json!("other"), other).await;
    poll_fn(|cx| {
        assert!(route.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let reply = replies
        .try_recv()
        .expect("the other session must reply while inventory is held");
    assert_eq!(
        *reply,
        json!({"jsonrpc":"2.0","id":"other","result":{"items":[]}})
    );
}

#[tokio::test]
async fn both_restore_inventory_reads_expire_on_the_route_clock() {
    for phase in [InventoryRead::Validation, InventoryRead::Reply] {
        let mut connection = RestoreConnection::new();
        let clock = connection.fixture().clock.clone();
        let (reading, release) = connection.fixture().hold_inventory(phase);
        match phase {
            InventoryRead::Validation => {
                request(
                    &connection.input,
                    "session/load",
                    json!("first"),
                    &connection.backend.session,
                )
                .await;
            }
            InventoryRead::Reply => {
                connection.hold_restore("session/resume").await;
                connection.release.send(()).expect("release activation");
            }
        }
        reaches_inventory(&mut connection.route, reading).await;

        clock
            .advance(Duration::from_secs(10))
            .expect("reach the inventory deadline");
        let reply = next_response(&mut connection.route, &mut connection.replies).await;
        assert_eq!(
            reply,
            json!({
                "jsonrpc":"2.0", "id":"first",
                "error":{"code":-32000,"message":"the store didn't respond in time"}
            })
        );
        assert!(
            release.is_closed(),
            "the route dropped its expired inventory read"
        );
        let fixture = connection
            .backend
            .restore
            .as_ref()
            .expect("restore fixture");
        let calls = fixture.calls.lock().expect("restore calls").len();
        assert_eq!(calls, usize::from(matches!(phase, InventoryRead::Reply)));
        connection.closed.cancel();
        connection.route.await.expect("route shutdown");
    }
}
