//! Exercise the Host's selected policy with the native pump. Only transport and
//! store ports stand in; physical I/O and SQLite durability have separate cases.
use super::*;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardReadPage, BoardReadScope, NoticeRead,
};
use fleet_sync_protocol::{
    AuthRefusal, ClientFrame, OperationAnswer, OperationRequest, ServerFrame, SessionId,
};
use fleet_sync_session::{
    Bytes, ClientError, ClientSessionState, ConnectFailure, ConnectionClosed, Endpoint,
    PeerCredential, SessionClientStore, SessionTransport, StoreError, TransportConnection,
};
use fleet_sync_tokio_runner::SessionLinkEventKind;
use runtime_primitives::{Clock, SyncTime};
use std::{collections::VecDeque, sync::Mutex};
use tokio::time::{Instant, advance, timeout};

#[derive(Clone, Default)]
struct MemorySessionStore(Arc<Mutex<Option<ClientSessionState>>>);
impl SessionClientStore for MemorySessionStore {
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        *self.0.lock().unwrap() = Some(state.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct PausedClock(Instant);
impl Clock for PausedClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(i64::try_from(self.0.elapsed().as_millis()).unwrap())
    }
}

enum HelloReply {
    Welcome,
    Ended,
    Silent,
}
enum ConnectStep {
    Refused(AuthRefusal),
    Accepted(HelloReply),
}
struct ScriptedTransport {
    steps: Mutex<VecDeque<ConnectStep>>,
    attempts: mpsc::UnboundedSender<Instant>,
    hellos: mpsc::UnboundedSender<SessionId>,
}
impl SessionTransport for ScriptedTransport {
    type Connection = ScriptedConnection;
    async fn connect(
        &self,
        _: &Endpoint,
        _: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        self.attempts.send(Instant::now()).unwrap();
        match self.steps.lock().unwrap().pop_front() {
            Some(ConnectStep::Refused(reason)) => Err(ConnectFailure::CredentialRejected(reason)),
            Some(ConnectStep::Accepted(reply)) => {
                let (send, receive) = mpsc::unbounded_channel();
                Ok(ScriptedConnection {
                    reply,
                    send,
                    receive,
                    hellos: self.hellos.clone(),
                })
            }
            None => Err(ConnectFailure::Unavailable),
        }
    }
}
struct ScriptedConnection {
    reply: HelloReply,
    send: mpsc::UnboundedSender<Bytes>,
    receive: mpsc::UnboundedReceiver<Bytes>,
    hellos: mpsc::UnboundedSender<SessionId>,
}
impl TransportConnection for ScriptedConnection {
    async fn send(&mut self, bytes: Bytes) -> Result<(), ConnectionClosed> {
        let frame: ClientFrame<BoardConsumerProtocol> = serde_json::from_slice(&bytes).unwrap();
        let response: ServerFrame<BoardConsumerProtocol> = match frame {
            ClientFrame::Hello { session_id, .. } => {
                self.hellos.send(session_id).unwrap();
                match self.reply {
                    HelloReply::Welcome => ServerFrame::Welcome {
                        session_id,
                        databases: Vec::new(),
                    },
                    HelloReply::Ended => ServerFrame::Closed {
                        reason: fleet_sync_protocol::CloseReason::SessionEnded,
                    },
                    HelloReply::Silent => return Ok(()),
                }
            }
            ClientFrame::Request { frame, body } => {
                assert!(matches!(
                    body,
                    OperationRequest::Consumer(BoardConsumerRequest::Read {
                        scope: BoardReadScope::Notices,
                        cursor: None
                    })
                ));
                ServerFrame::Answer {
                    frame,
                    body: OperationAnswer::Consumer(BoardConsumerAnswer::Read(
                        BoardReadPage::Notices(NoticeRead::Page {
                            notices: Vec::new(),
                            as_of: chrono::Utc::now(),
                        }),
                    )),
                }
            }
            ClientFrame::Answer { .. } => return Err(ConnectionClosed::UnsupportedFrame),
        };
        self.send
            .send(serde_json::to_vec(&response).unwrap().into())
            .unwrap();
        Ok(())
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        self.receive.recv().await.map(Ok)
    }
    async fn close(self, _: fleet_sync_protocol::CloseReason) {}
}

type RecoveryPump = SessionPump<ScriptedTransport, MemorySessionStore, BoardConsumerProtocol>;
struct RecoveryFixture {
    requests: SessionRequests<BoardConsumerProtocol>,
    events: mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>,
    pump: RecoveryPump,
    attempts: mpsc::UnboundedReceiver<Instant>,
    hellos: mpsc::UnboundedReceiver<SessionId>,
    store: MemorySessionStore,
    original: SessionId,
}
async fn prepare_recovery(steps: Vec<ConnectStep>) -> RecoveryFixture {
    let (attempted, attempts) = mpsc::unbounded_channel();
    let (hello, hellos) = mpsc::unbounded_channel();
    let store = MemorySessionStore::default();
    let client = SessionClient::load(
        ScriptedTransport {
            steps: Mutex::new(steps.into()),
            attempts: attempted,
            hellos: hello,
        },
        store.clone(),
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
            factor: 2,
        },
    )
    .await
    .unwrap();
    let original = client.state().session_id;
    let (requests, events, pump) = create_machine_session_pump(
        client,
        Endpoint::new("ws://fixture/session").unwrap(),
        PeerCredential::new("Bearer fixture").unwrap(),
    );
    RecoveryFixture {
        requests,
        events,
        pump,
        attempts,
        hellos,
        store,
        original,
    }
}
async fn assert_notice_request_succeeds(requests: &SessionRequests<BoardConsumerProtocol>) {
    let answer = requests
        .request(OperationRequest::Consumer(BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor: None,
        }))
        .await
        .unwrap();
    assert!(
        matches!(answer, OperationAnswer::Consumer(BoardConsumerAnswer::Read(BoardReadPage::Notices(NoticeRead::Page { notices, .. }))) if notices.is_empty())
    );
}
async fn await_welcome(events: &mut mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>) {
    timeout(Duration::from_secs(1), async {
        loop {
            match events.recv().await {
                Some(ClientEvent::Welcome { .. }) => break,
                Some(_) => {}
                None => panic!("pump ended before Welcome"),
            }
        }
    })
    .await
    .unwrap();
}

#[tokio::test(start_paused = true)]
async fn host_policy_retries_expired_credentials_from_one_second_to_five_minutes_with_the_same_requests()
 {
    let mut steps: Vec<_> = (0..10)
        .map(|_| ConnectStep::Refused(AuthRefusal::Expired))
        .collect();
    steps.push(ConnectStep::Accepted(HelloReply::Welcome));
    let RecoveryFixture {
        requests,
        mut events,
        pump,
        mut attempts,
        store,
        original,
        hellos: _hellos,
    } = prepare_recovery(steps).await;
    let mut links = pump.observe_link();
    let (stop, stopped) = watch::channel(false);
    let start = Instant::now();
    let task = tokio::spawn(pump.run_with_clock(stopped, PausedClock(start)));
    assert_eq!(attempts.recv().await.unwrap(), start);
    let mut due = start;
    for seconds in [1, 2, 4, 8, 16, 32, 64, 128, 256, 300] {
        loop {
            if matches!(
                links.recv().await.unwrap().kind,
                SessionLinkEventKind::CredentialRejected {
                    reason: AuthRefusal::Expired
                }
            ) {
                break;
            }
        }
        assert!(
            !requests.is_closed(),
            "credential retry closed the retained request handle"
        );
        advance(Duration::from_secs(seconds - 1)).await;
        tokio::task::yield_now().await;
        assert!(
            attempts.try_recv().is_err(),
            "credential retry bypassed its backoff"
        );
        advance(Duration::from_secs(1)).await;
        due += Duration::from_secs(seconds);
        assert_eq!(
            timeout(Duration::from_millis(100), attempts.recv())
                .await
                .unwrap()
                .unwrap(),
            due
        );
    }
    await_welcome(&mut events).await;
    assert_notice_request_succeeds(&requests).await;
    assert_eq!(store.load().await.unwrap().unwrap().session_id, original);
    stop.send_replace(true);
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn host_policy_renews_an_ended_pre_welcome_session_and_preserves_request_handles() {
    let RecoveryFixture {
        requests,
        mut events,
        pump,
        mut attempts,
        mut hellos,
        store,
        original,
    } = prepare_recovery(vec![
        ConnectStep::Accepted(HelloReply::Ended),
        ConnectStep::Accepted(HelloReply::Welcome),
    ])
    .await;
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(pump.run_with_clock(stopped, PausedClock(Instant::now())));
    let _first_attempt = attempts.recv().await.unwrap();
    assert_eq!(hellos.recv().await.unwrap(), original);
    assert!(matches!(
        events.recv().await,
        Some(ClientEvent::Disconnected { .. })
    ));
    assert!(!requests.is_closed());
    advance(Duration::from_secs(1)).await;
    let _second_attempt = timeout(Duration::from_millis(100), attempts.recv())
        .await
        .unwrap()
        .unwrap();
    let replacement = hellos.recv().await.unwrap();
    assert_ne!(replacement, original, "ended session identity was reused");
    await_welcome(&mut events).await;
    assert_eq!(store.load().await.unwrap().unwrap().session_id, replacement);
    assert_notice_request_succeeds(&requests).await;
    stop.send_replace(true);
    task.await.unwrap().unwrap();
}

#[tokio::test(start_paused = true)]
async fn host_policy_keeps_revoked_and_wrong_kind_terminal_with_typed_results() {
    for reason in [AuthRefusal::Revoked, AuthRefusal::WrongKind] {
        let RecoveryFixture {
            requests,
            pump,
            mut attempts,
            ..
        } = prepare_recovery(vec![ConnectStep::Refused(reason)]).await;
        let (_stop, stopped) = watch::channel(false);
        let result = pump
            .run_with_clock(stopped, PausedClock(Instant::now()))
            .await;
        assert!(matches!(
            (reason, result),
            (AuthRefusal::Revoked, Err(ClientError::Revoked))
                | (
                    AuthRefusal::WrongKind,
                    Err(ClientError::CredentialRejected {
                        reason: AuthRefusal::WrongKind
                    })
                )
        ));
        assert!(requests.is_closed());
        assert!(attempts.try_recv().is_ok());
        assert!(attempts.try_recv().is_err(), "terminal refusal reconnected");
    }
}

#[tokio::test(start_paused = true)]
async fn host_policy_bounds_welcome_to_thirty_seconds_before_reconnecting() {
    let RecoveryFixture {
        requests,
        mut events,
        pump,
        mut attempts,
        hellos: _hellos,
        ..
    } = prepare_recovery(vec![
        ConnectStep::Accepted(HelloReply::Silent),
        ConnectStep::Accepted(HelloReply::Welcome),
    ])
    .await;
    let mut links = pump.observe_link();
    let (stop, stopped) = watch::channel(false);
    let start = Instant::now();
    let task = tokio::spawn(pump.run_with_clock(stopped, PausedClock(start)));
    assert_eq!(attempts.recv().await.unwrap(), start);
    assert!(matches!(
        links.recv().await.unwrap().kind,
        SessionLinkEventKind::Connecting { attempt: 1 }
    ));
    advance(Duration::from_secs(29)).await;
    tokio::task::yield_now().await;
    assert!(links.try_recv().is_err());
    advance(Duration::from_secs(1)).await;
    assert!(matches!(
        timeout(Duration::from_millis(100), links.recv())
            .await
            .unwrap()
            .unwrap()
            .kind,
        SessionLinkEventKind::WelcomeTimeout
    ));
    assert!(!requests.is_closed());
    advance(Duration::from_secs(1)).await;
    assert_eq!(
        timeout(Duration::from_millis(100), attempts.recv())
            .await
            .unwrap()
            .unwrap(),
        start + Duration::from_secs(31)
    );
    await_welcome(&mut events).await;
    assert_notice_request_succeeds(&requests).await;
    stop.send_replace(true);
    task.await.unwrap().unwrap();
}

#[tokio::test]
async fn cooccurring_persistence_failure_is_not_hidden_by_an_expected_refusal_in_either_order() {
    for refusal_first in [true, false] {
        let (refuse, refusal_ready) = tokio::sync::oneshot::channel();
        let (persist, persistence_ready) = tokio::sync::oneshot::channel();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            refusal_ready.await.unwrap();
            Err::<(), _>(io::Error::other(ClientError::Revoked))
        });
        tasks.spawn(async move {
            persistence_ready.await.unwrap();
            Err::<(), _>(io::Error::other(StoreError::Unavailable {
                message: "link persistence failed".to_owned(),
            }))
        });
        let (first, second) = if refusal_first {
            (refuse, persist)
        } else {
            (persist, refuse)
        };
        first.send(()).unwrap();
        let mut failure = Some(tasks.join_next().await.unwrap().unwrap().unwrap_err());
        second.send(()).unwrap();
        drain_session_tasks(&mut tasks, &mut failure).await;
        let failure = failure.unwrap();
        assert!(
            matches!(failure.get_ref().and_then(|error| error.downcast_ref::<StoreError>()),
            Some(StoreError::Unavailable { message }) if message == "link persistence failed"),
            "refusal hid a co-occurring persistence failure: {failure}"
        );
        assert!(!expected_terminal_io(&failure));
        let observed = board_sync_protocol::FleetLinkRecord {
            peer: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
            endpoint: "wss://fixture/session".to_owned(),
            state: board_sync_protocol::FleetLinkState::Refused {
                reason: board_sync_protocol::FleetLinkRefusal::CredentialRejected {
                    reason: AuthRefusal::Revoked,
                },
                at: chrono::Utc::now(),
            },
            last_drop: None,
            last_failure: None,
        };
        assert!(!RunningMachineSession::is_observed_refusal(
            &failure, &observed
        ));
    }
}
