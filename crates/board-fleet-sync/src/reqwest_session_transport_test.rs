use super::*;
use fleet_protocol::{ClientFrame, NoConsumer, SchemaSupport, ServerFrame};
use fleet_session::{
    ClientEvent, ClientSessionState, SessionClient, SessionClientStore, StoreError,
};
use fleet_session_client::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
use tokio::sync::{mpsc, watch};

async fn poll_once<F: std::future::Future>(mut future: Pin<&mut F>) -> Poll<F::Output> {
    std::future::poll_fn(|context| Poll::Ready(future.as_mut().poll(context))).await
}

struct ControlledSocket {
    incoming: mpsc::UnboundedReceiver<Result<Message, ()>>,
    frames: mpsc::UnboundedSender<Result<Message, ()>>,
    outgoing: mpsc::UnboundedSender<Message>,
    welcome: bool,
    send_pending: bool,
}

impl Stream for ControlledSocket {
    type Item = Result<Message, ()>;
    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.incoming.poll_recv(context)
    }
}

impl Sink<Message> for ControlledSocket {
    type Error = ();
    fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        if self.send_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
    fn start_send(self: Pin<&mut Self>, message: Message) -> Result<(), ()> {
        if self.welcome
            && let Message::Binary(bytes) = &message
            && let Ok(ClientFrame::<NoConsumer>::Hello { session_id, .. }) =
                serde_json::from_slice(bytes)
        {
            let welcome = ServerFrame::<NoConsumer>::Welcome {
                capabilities: None,
                session_id,
                databases: Vec::new(),
            };
            self.frames
                .send(Ok(Message::Binary(
                    serde_json::to_vec(&welcome).map_err(|_| ())?.into(),
                )))
                .map_err(|_| ())?;
        }
        self.outgoing.send(message).map_err(|_| ())
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
        Poll::Ready(Ok(()))
    }
}

fn controlled_connection() -> (
    SessionSocket<ControlledSocket>,
    mpsc::UnboundedSender<Result<Message, ()>>,
    mpsc::UnboundedReceiver<Message>,
) {
    let (frames, incoming) = mpsc::unbounded_channel();
    let (outgoing, sent) = mpsc::unbounded_channel();
    let connection = SessionSocket::new(
        ControlledSocket {
            incoming,
            frames: frames.clone(),
            outgoing,
            welcome: true,
            send_pending: false,
        },
        Duration::from_secs(5),
    );
    (connection, frames, sent)
}

#[tokio::test(start_paused = true)]
async fn silent_socket_closes_at_the_idle_bound() {
    // No incoming frame at all, even though writes keep succeeding.
    let (mut connection, _frames, _sent) = controlled_connection();
    let result = timeout(Duration::from_secs(61), connection.receive()).await;
    assert_eq!(result, Ok(Some(Err(ConnectionClosed::IdleTimeout))));
    assert_eq!(connection.receive().await, None);
}

#[tokio::test(start_paused = true)]
async fn binary_send_timeout_retains_its_native_reason_and_invalidates_the_socket() {
    let (mut connection, _frames, _sent) = controlled_connection();
    connection.socket.as_mut().unwrap().send_pending = true;
    assert_eq!(
        connection.send(Bytes::from_static(b"outgoing")).await,
        Err(ConnectionClosed::SendTimeout)
    );
    assert_eq!(connection.receive().await, None);
}

#[tokio::test(start_paused = true)]
async fn ping_send_timeout_retains_its_native_reason_and_invalidates_the_socket() {
    let (mut connection, _frames, _sent) = controlled_connection();
    connection.socket.as_mut().unwrap().send_pending = true;
    assert_eq!(
        timeout(Duration::from_secs(26), connection.receive()).await,
        Ok(Some(Err(ConnectionClosed::SendTimeout)))
    );
    assert_eq!(connection.receive().await, None);
}

#[tokio::test(start_paused = true)]
async fn ping_period_and_idle_bound_survive_cancelled_receives_and_outgoing_frames() {
    let (mut connection, _frames, mut sent) = controlled_connection();
    for second in 0..60 {
        connection
            .send(Bytes::from_static(b"outgoing"))
            .await
            .unwrap();
        assert!(matches!(sent.try_recv(), Ok(Message::Binary(_))));
        let receiving = connection.receive();
        tokio::pin!(receiving);
        assert!(poll_once(receiving.as_mut()).await.is_pending());
        if second > 0 && second % 20 == 0 {
            assert!(matches!(sent.try_recv(), Ok(Message::Ping(_))));
        } else {
            assert!(sent.try_recv().is_err());
        }
        tokio::time::advance(Duration::from_secs(1)).await;
    }
    assert_eq!(
        connection.receive().await,
        Some(Err(ConnectionClosed::IdleTimeout))
    );
    assert!(sent.try_recv().is_err());
}

#[tokio::test(start_paused = true)]
async fn incoming_binary_ping_and_pong_each_extend_the_idle_bound() {
    for frame in [
        Message::Binary(Bytes::from_static(b"contact")),
        Message::Ping(Bytes::new()),
        Message::Pong(Bytes::new()),
    ] {
        let (mut connection, frames, _sent) = controlled_connection();
        for _ in 0..4 {
            let receiving = connection.receive();
            tokio::pin!(receiving);
            assert!(poll_once(receiving.as_mut()).await.is_pending());
            tokio::time::advance(Duration::from_secs(40)).await;
            frames.send(Ok(frame.clone())).unwrap();
            match &frame {
                Message::Binary(bytes) => assert_eq!(
                    poll_once(receiving.as_mut()).await,
                    Poll::Ready(Some(Ok(bytes.clone())))
                ),
                _ => assert!(poll_once(receiving.as_mut()).await.is_pending()),
            }
        }
        assert_eq!(
            timeout(Duration::from_secs(61), connection.receive()).await,
            Ok(Some(Err(ConnectionClosed::IdleTimeout)))
        );
    }
}

#[derive(Clone, Default)]
struct MemoryStore(Arc<Mutex<Option<ClientSessionState>>>);
impl SessionClientStore for MemoryStore {
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        *self.0.lock().unwrap() = Some(state.clone());
        Ok(())
    }
}

type ControlledSocketChannels = (
    mpsc::UnboundedReceiver<Message>,
    mpsc::UnboundedSender<Result<Message, ()>>,
);

struct ControlledTransport {
    connected: mpsc::UnboundedSender<ControlledSocketChannels>,
    welcome: bool,
}
impl SessionTransport for ControlledTransport {
    type Connection = SessionSocket<ControlledSocket>;
    async fn connect(
        &self,
        _: &Endpoint,
        _: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        let (mut connection, frames, outgoing) = controlled_connection();
        connection.socket.as_mut().unwrap().welcome = self.welcome;
        self.connected.send((outgoing, frames)).unwrap();
        Ok(connection)
    }
}

#[tokio::test(start_paused = true)]
async fn session_pump_reconnects_after_a_silent_socket() {
    let (connected, mut connections) = mpsc::unbounded_channel();
    let client = SessionClient::<_, _, NoConsumer>::load(
        ControlledTransport {
            connected,
            welcome: true,
        },
        MemoryStore::default(),
        &UuidV7Ids,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: Duration::ZERO,
            max: Duration::ZERO,
            factor: 2,
        },
    )
    .await
    .unwrap();
    let (_requests, mut events, pump) = SessionPump::new(
        client,
        Endpoint::new("ws://fixture/session").unwrap(),
        PeerCredential::new("Bearer fixture").unwrap(),
        Duration::from_secs(5),
    );
    let (stop, stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run(stopped));
    let _first = connections.recv().await.unwrap();
    assert!(matches!(
        events.recv().await,
        Some(ClientEvent::Welcome { .. })
    ));
    assert!(matches!(
        timeout(Duration::from_secs(61), events.recv()).await,
        Ok(Some(ClientEvent::Disconnected { .. }))
    ));
    let _second = connections.recv().await.unwrap();
    assert!(matches!(
        events.recv().await,
        Some(ClientEvent::Welcome { .. })
    ));
    stop.send_replace(true);
    pumping.await.unwrap().unwrap();
}

#[derive(Clone)]
struct PausedClock(Instant);
impl runtime_primitives::Clock for PausedClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(
            i64::try_from(self.0.elapsed().as_millis()).unwrap(),
        )
    }
}

#[tokio::test(start_paused = true)]
async fn native_welcome_timeout_closes_a_socket_despite_regular_pongs() {
    use fleet_session_client::{SessionLinkEventKind, SessionRecoveryPolicy};
    let (connected, mut connections) = mpsc::unbounded_channel();
    let client = SessionClient::<_, _, NoConsumer>::load(
        ControlledTransport {
            connected,
            welcome: false,
        },
        MemoryStore::default(),
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
    let (_requests, _events, pump) = SessionPump::new(
        client,
        Endpoint::new("ws://fixture/session").unwrap(),
        PeerCredential::new("Bearer fixture").unwrap(),
        Duration::from_secs(5),
    );
    let pump = pump.with_recovery_policy(SessionRecoveryPolicy {
        welcome_timeout: Some(Duration::from_secs(30)),
        ..SessionRecoveryPolicy::default()
    });
    let mut links = pump.observe_link();
    let (stop, stopped) = watch::channel(false);
    let pumping = tokio::spawn(pump.run_with_clock(stopped, PausedClock(Instant::now())));
    let (mut outgoing, frames) = connections.recv().await.unwrap();
    assert!(matches!(outgoing.recv().await, Some(Message::Binary(_))));
    assert_eq!(
        links.recv().await.unwrap().kind,
        SessionLinkEventKind::Connecting { attempt: 1 }
    );
    for _ in 0..29 {
        frames.send(Ok(Message::Pong(Bytes::new()))).unwrap();
        tokio::task::yield_now().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            links.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    }
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(
        links.recv().await.unwrap().kind,
        SessionLinkEventKind::WelcomeTimeout
    );
    let mut close_observed = false;
    while let Some(frame) = outgoing.recv().await {
        if matches!(frame, Message::Close { .. }) {
            close_observed = true;
            break;
        }
    }
    assert!(
        close_observed,
        "Welcome timeout must close the physical socket"
    );
    assert!(
        connections.try_recv().is_err(),
        "reconnect must honor backoff"
    );
    stop.send_replace(true);
    pumping.await.unwrap().unwrap();
}
