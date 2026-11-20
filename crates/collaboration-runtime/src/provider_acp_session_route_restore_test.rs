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

pub(super) struct RestoreFixture {
    pub(super) other: SessionRef,
    pub(super) state: Mutex<SessionState>,
    pub(super) queue_reads: Mutex<Vec<SessionRef>>,
    calls: Mutex<Vec<(RestoreMethod, SessionRef)>>,
    clock: ManualClock,
    gate: Mutex<Option<RestoreGate>>,
}

impl RestoreFixture {
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
            router_clock::timeout(&self.clock, Duration::from_secs(30), gate.release)
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
        Arc::get_mut(&mut backend)
            .expect("fixture owns the backend")
            .restore = Some(RestoreFixture {
            other,
            state: Mutex::new(SessionState::Unloaded),
            queue_reads: Mutex::default(),
            calls: Mutex::default(),
            clock: ManualClock::new(chrono::DateTime::UNIX_EPOCH),
            gate: Mutex::new(Some(RestoreGate {
                entered: report_entered,
                release: wait_for_release,
            })),
        });
        let closed = CancellationToken::new();
        let (input, input_frames) = bounded_acp_output(closed.clone());
        let (output, replies) = bounded_acp_output(closed.clone());
        let route = Box::new(ProviderAcpSessionRoute::new(
            backend.session.endpoint.clone(),
            backend.clone(),
            backend.clone(),
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
        tokio::select! {
            entered = &mut self.entered => entered.expect("host restore handshake"),
            result = &mut self.route => panic!("route exited before entering the restore: {result:?}"),
        }
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
