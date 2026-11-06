use super::*;
use fleet_sync_protocol::{ClientFrame, NoConsumer, SchemaSupport, ServerFrame};
use fleet_sync_session::{
    ClientEvent, ClientSessionState, SessionClient, SessionClientStore, StoreError,
};
use fleet_sync_tokio_runner::SessionPump;
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
        Poll::Ready(Ok(()))
    }
    fn start_send(self: Pin<&mut Self>, message: Message) -> Result<(), ()> {
        if let Message::Binary(bytes) = &message
            && let Ok(ClientFrame::<NoConsumer>::Hello { session_id, .. }) =
                serde_json::from_slice(bytes)
        {
            let welcome = ServerFrame::<NoConsumer>::Welcome {
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
    assert_eq!(result, Ok(Some(Err(ConnectionClosed::Transport))));
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
        Some(Err(ConnectionClosed::Transport))
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
            Ok(Some(Err(ConnectionClosed::Transport)))
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

struct ControlledTransport {
    connected: mpsc::UnboundedSender<mpsc::UnboundedReceiver<Message>>,
}
impl SessionTransport for ControlledTransport {
    type Connection = SessionSocket<ControlledSocket>;
    async fn connect(
        &self,
        _: &Endpoint,
        _: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        let (connection, _frames, outgoing) = controlled_connection();
        self.connected.send(outgoing).unwrap();
        Ok(connection)
    }
}

#[tokio::test(start_paused = true)]
async fn session_pump_reconnects_after_a_silent_socket() {
    let (connected, mut connections) = mpsc::unbounded_channel();
    let client = SessionClient::<_, _, NoConsumer>::load(
        ControlledTransport { connected },
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
