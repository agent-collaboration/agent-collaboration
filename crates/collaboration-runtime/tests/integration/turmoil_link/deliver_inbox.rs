//! A real Router driver and its SQLite inbox over a simulated socket.
use central_link_client::{FlushOutcomeSink, FlushOutcomes};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, MachineTokenText, TokenRenewalResult, decode_text,
    encode_text,
};
use collaboration_link_test_support::{
    central_request_log::{CENTRAL_HOST, LINK_PORT},
    router_host::{INSTALLATION, ROUTER_HOST},
    simulated_clock::SimulatedClock,
    turmoil_transport::TurmoilLinkTransport,
};
use collaboration_runtime::{
    CentralHandoffUnavailable, CentralInbox, CentralLinkParts, CentralMessageHandoff, FlushJitter,
    MachineTokenSource, MemoryOutbox, RouterLinkCalls, TokenAvailability, TransportFuture,
};
use futures_util::{SinkExt, StreamExt};
use router_clock::RuntimeClock;
use runtime_primitives::SyncTime;
use std::{
    cell::RefCell,
    net::{IpAddr, Ipv4Addr},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};
use tokio::{sync::Mutex, task::JoinSet};
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use turmoil::net::{TcpListener, TcpStream};
use workspace_model::central_mail::{DeliveryAnswer, MessageRecord};

const FIRST_PUSH: Duration = Duration::from_secs(20);
const TEST_DEADLINE: Duration = Duration::from_secs(240);

#[derive(Default)]
struct Observations {
    connections: usize,
    pushes: usize,
    acks: Vec<(Duration, DeliveryAnswer)>,
    discarded_ack_at: Option<Duration>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LinkFault {
    PartitionBeforePush,
    DisconnectBeforeSettlingAck,
}

#[derive(Default)]
struct Handoffs(AtomicUsize);

impl CentralMessageHandoff for Handoffs {
    fn handoff<'call>(
        &'call self,
        _message: &'call MessageRecord,
    ) -> TransportFuture<'call, Result<DeliveryAnswer, CentralHandoffUnavailable>> {
        Box::pin(async move {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(DeliveryAnswer::Accepted)
        })
    }
}

struct RouterFixture;

impl MachineTokenSource for RouterFixture {
    fn availability(&self) -> TokenAvailability {
        TokenAvailability::Ready(
            MachineTokenText::try_from("simulated.machine.token".to_owned()).unwrap(),
        )
    }
    fn changed(&self) -> TransportFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
    fn report_renewal(&self, _: &TokenRenewalResult) {}
}

impl FlushJitter for RouterFixture {
    fn jitter_millis(&self, _: SyncTime) -> i64 {
        0
    }
}

impl FlushOutcomeSink for RouterFixture {
    fn deliver(&self, _: FlushOutcomes) {}
    fn ended(&self, _: Vec<central_link_client::EndedWrite>) {}
}

fn fixture_message() -> MessageRecord {
    serde_json::from_value(serde_json::json!({
        "messageId": "01890f2e-7b4c-7cc0-98c4-000000000009",
        "sender": {"kind": "central"},
        "target": {
            "machineId": INSTALLATION, "endpoint": "codex-local", "sessionId": "recipient"
        },
        "body": {"kind": "direct", "body": "hello", "delivery": "auto", "replyTo": null},
        "state": {"kind": "queued"},
        "source": null,
        "createdAt": "2030-01-01T00:00:00Z",
        "updatedAt": "2030-01-01T00:00:00Z",
    }))
    .unwrap()
}

async fn serve(observed: Rc<RefCell<Observations>>, fault: LinkFault) -> turmoil::Result {
    let listener = TcpListener::bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), LINK_PORT)).await?;
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            Some(finished) = connections.join_next(), if !connections.is_empty() => finished??,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.spawn_local(serve_delivery(stream, observed.clone(), fault));
            }
        }
    }
}

async fn serve_delivery(
    stream: TcpStream,
    observed: Rc<RefCell<Observations>>,
    fault: LinkFault,
) -> turmoil::Result {
    let Ok(mut socket) = accept_async(stream).await else {
        return Ok(());
    };
    let clock = SimulatedClock::for_host(CENTRAL_HOST);
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: INSTALLATION.parse().unwrap(),
        central_time: clock.utc_now(),
    });
    if send(&mut socket, &welcome).await.is_err() {
        return Ok(());
    }
    let connection = {
        let mut observed = observed.borrow_mut();
        observed.connections += 1;
        observed.connections
    };
    if connection == 1 {
        clock
            .sleep_until(router_clock::ClockInstant::ZERO.after(FIRST_PUSH))
            .await;
    }
    let message = fixture_message();
    if observed.borrow().acks.is_empty() {
        observed.borrow_mut().pushes += 1;
        if send(
            &mut socket,
            &LinkFrame::Data(DataFrame::Deliver {
                message: Box::new(message.clone()),
            }),
        )
        .await
        .is_err()
        {
            return Ok(());
        }
    }
    while let Some(Ok(Message::Text(text))) = socket.next().await {
        match decode_text(text.as_str(), FrameDirection::ToCentral)? {
            LinkFrame::Ping => {
                if send(&mut socket, &LinkFrame::Pong).await.is_err() {
                    break;
                }
            }
            LinkFrame::Data(DataFrame::Ack { message_id, answer }) => {
                assert_eq!(message_id, message.message_id);
                let mut log = observed.borrow_mut();
                if fault == LinkFault::DisconnectBeforeSettlingAck && log.discarded_ack_at.is_none()
                {
                    log.discarded_ack_at = Some(turmoil::elapsed());
                    // The frame crossed the socket, but this connection ends
                    // before the answer settles. Welcome resends the message.
                    return Ok(());
                }
                log.acks.push((turmoil::elapsed(), answer));
            }
            other => return Err(format!("unexpected Router frame: {other:?}").into()),
        }
    }
    Ok(())
}

async fn send(socket: &mut WebSocketStream<TcpStream>, frame: &LinkFrame) -> turmoil::Result {
    let text = encode_text(frame, FrameDirection::ToRouter)?;
    socket.send(Message::text(text)).await?;
    Ok(())
}

fn run_delivery(fault: LinkFault) {
    let observed = Rc::new(RefCell::new(Observations::default()));
    let server = Rc::clone(&observed);
    let client_observations = Rc::clone(&observed);
    let handoffs = Arc::new(Handoffs::default());
    let client_handoffs = Arc::clone(&handoffs);
    let root = tempfile::tempdir().unwrap();
    let mut sim = turmoil::Builder::new()
        .rng_seed(17)
        .epoch(SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_456_000))
        .tick_duration(Duration::from_millis(20))
        .simulation_duration(TEST_DEADLINE)
        .build();
    sim.host(CENTRAL_HOST, move || serve(server.clone(), fault));
    sim.client(ROUTER_HOST, async move {
        let clock = Arc::new(SimulatedClock::for_host(ROUTER_HOST));
        let inbox =
            CentralInbox::open(&root.path().join("machine_local.sqlite"), client_handoffs).await?;
        let tasks = TaskTracker::new();
        let stop = CancellationToken::new();
        let calls = RouterLinkCalls::start(
            CentralLinkParts {
                clock: clock.clone(),
                installation: INSTALLATION.parse()?,
                store: Arc::new(Mutex::new(MemoryOutbox::new())),
                transport: Arc::new(TurmoilLinkTransport::new(CENTRAL_HOST, LINK_PORT, "/link")),
                tokens: Arc::new(RouterFixture),
                jitter: Arc::new(RouterFixture),
                outcomes: Arc::new(RouterFixture),
                inbox: Some(inbox),
            },
            &tasks,
            stop.clone(),
        );
        while client_observations.borrow().acks.is_empty() {
            clock
                .sleep_until(clock.monotonic_now().after(Duration::from_secs(1)))
                .await;
        }
        drop(calls);
        stop.cancel();
        tasks.close();
        tasks.wait().await;
        Ok(())
    });
    let repair_at = Duration::from_secs(100);
    let mut cut = false;
    let mut repaired = false;
    loop {
        let now = sim.elapsed();
        if fault == LinkFault::PartitionBeforePush && !cut && now >= Duration::from_secs(15) {
            assert_eq!(handoffs.0.load(Ordering::SeqCst), 0);
            assert!(observed.borrow().acks.is_empty());
            sim.partition(ROUTER_HOST, CENTRAL_HOST);
            cut = true;
        }
        if cut && !repaired && now >= repair_at {
            assert!(observed.borrow().acks.is_empty());
            sim.repair(ROUTER_HOST, CENTRAL_HOST);
            repaired = true;
        }
        if sim.step().unwrap() {
            break;
        }
    }
    let observed = observed.borrow();
    assert!(observed.connections >= 2 && observed.pushes >= 2);
    assert_eq!(handoffs.0.load(Ordering::SeqCst), 1);
    let [(accepted_at, DeliveryAnswer::Accepted)] = observed.acks.as_slice() else {
        panic!("central never received its one Ack: {:?}", observed.acks);
    };
    assert_on_minute(*accepted_at);
    match fault {
        LinkFault::PartitionBeforePush => {
            assert!(cut && repaired);
            assert!(*accepted_at >= repair_at);
            assert_eq!(observed.discarded_ack_at, None);
        }
        LinkFault::DisconnectBeforeSettlingAck => {
            let lost_at = observed
                .discarded_ack_at
                .expect("the first Ack reached the socket");
            assert_on_minute(lost_at);
            assert!(*accepted_at >= lost_at + Duration::from_secs(54));
        }
    }
}

fn assert_on_minute(observed_at: Duration) {
    let remainder = observed_at.as_millis() % 60_000;
    assert!(remainder <= 3_000 || remainder >= 57_000);
}

#[test]
fn a_partition_across_delivery_waits_for_the_reconnect_push() {
    run_delivery(LinkFault::PartitionBeforePush);
}

#[test]
fn reconnect_resends_after_a_lost_ack_without_repeating_session_handoff() {
    run_delivery(LinkFault::DisconnectBeforeSettlingAck);
}
