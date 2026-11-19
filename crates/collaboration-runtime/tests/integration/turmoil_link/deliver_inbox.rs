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
    CentralHandoffUnavailable, CentralInbox, CentralLinkParts, CentralMessageHandoff,
    CentralRecipientLookup, CentralRecipientUnavailable, FlushJitter, MachineTokenSource,
    MemoryOutbox, RouterLinkCalls, TokenAvailability, TransportFuture,
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
use tokio::{
    sync::{Mutex, Notify},
    task::JoinSet,
};
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

struct Handoffs {
    count: AtomicUsize,
    wait_for_first_ack: Option<Arc<Notify>>,
}

impl CentralMessageHandoff for Handoffs {
    fn handoff<'call>(
        &'call self,
        _message: &'call MessageRecord,
    ) -> TransportFuture<'call, Result<(), CentralHandoffUnavailable>> {
        Box::pin(async move {
            if let Some(first_ack) = &self.wait_for_first_ack {
                first_ack.notified().await;
            }
            self.count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    }
}

struct RouterFixture;

impl CentralRecipientLookup for RouterFixture {
    fn answer<'call>(
        &'call self,
        _: &'call MessageRecord,
    ) -> TransportFuture<'call, Result<DeliveryAnswer, CentralRecipientUnavailable>> {
        Box::pin(std::future::ready(Ok(DeliveryAnswer::Accepted)))
    }
}

impl MachineTokenSource for RouterFixture {
    #[expect(clippy::unwrap_used, reason = "the fixed fixture token is well formed")]
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
    fn delivery_outcome(
        &self,
        _: &workspace_model::Caller,
        _: &central_link_client::SenderDeliveryOutcome,
    ) {
    }
}

#[expect(clippy::unwrap_used, reason = "the fixed fixture is a valid message")]
fn fixture_message() -> MessageRecord {
    serde_json::from_value(serde_json::json!({
        "messageId": "01890f2e-7b4c-7cc0-98c4-000000000009",
        "sender": {"kind": "central"},
        "target": {
            "machineId": INSTALLATION, "sessionId": "recipient"
        },
        "body": {"kind": "direct", "body": "hello", "delivery": "auto", "replyTo": null},
        "state": {"kind": "queued"},
        "source": null,
        "createdAt": "2030-01-01T00:00:00Z",
        "updatedAt": "2030-01-01T00:00:00Z",
    }))
    .unwrap()
}

async fn serve(
    observed: Rc<RefCell<Observations>>,
    fault: LinkFault,
    first_ack: Arc<Notify>,
) -> turmoil::Result {
    let listener = TcpListener::bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), LINK_PORT)).await?;
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            Some(finished) = connections.join_next(), if !connections.is_empty() => finished??,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                connections.spawn_local(serve_delivery(stream, observed.clone(), fault, first_ack.clone()));
            }
        }
    }
}

async fn serve_delivery(
    stream: TcpStream,
    observed: Rc<RefCell<Observations>>,
    fault: LinkFault,
    first_ack: Arc<Notify>,
) -> turmoil::Result {
    let Ok(mut socket) = accept_async(stream).await else {
        return Ok(());
    };
    let clock = SimulatedClock::for_host(CENTRAL_HOST);
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: INSTALLATION.parse()?,
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
            .sleep_until(clock.monotonic_now().after(FIRST_PUSH))
            .await;
    }
    let message = fixture_message();
    if observed.borrow().acks.is_empty()
        && (fault == LinkFault::PartitionBeforePush || connection == 1)
    {
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
                    first_ack.notify_one();
                    // The frame crossed the socket, but this connection ends
                    // before the answer settles. The next socket sends only
                    // Welcome, so it cannot re-arm the answer via Deliver.
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

#[expect(
    clippy::panic_in_result_fn,
    reason = "assertions report which simulated delivery obligation failed"
)]
fn run_delivery(fault: LinkFault) -> turmoil::Result {
    let observed = Rc::new(RefCell::new(Observations::default()));
    let server = Rc::clone(&observed);
    let client_observations = Rc::clone(&observed);
    let first_ack = Arc::new(Notify::new());
    let handoffs = Arc::new(Handoffs {
        count: AtomicUsize::new(0),
        wait_for_first_ack: (fault == LinkFault::DisconnectBeforeSettlingAck)
            .then(|| first_ack.clone()),
    });
    let client_handoffs = Arc::clone(&handoffs);
    let root = tempfile::tempdir()?;
    let mut sim = turmoil::Builder::new()
        .rng_seed(17)
        .epoch(SystemTime::UNIX_EPOCH + Duration::from_secs(1_893_456_000))
        .tick_duration(Duration::from_millis(20))
        .simulation_duration(TEST_DEADLINE)
        .build();
    sim.host(CENTRAL_HOST, move || {
        serve(server.clone(), fault, first_ack.clone())
    });
    sim.client(ROUTER_HOST, async move {
        let clock = Arc::new(SimulatedClock::for_host(ROUTER_HOST));
        let inbox = CentralInbox::open(
            &root.path().join("machine_local.sqlite"),
            Arc::new(RouterFixture),
            Some(client_handoffs.clone()),
        )
        .await?;
        let tasks = TaskTracker::new();
        let stop = CancellationToken::new();
        let calls = RouterLinkCalls::start(
            CentralLinkParts {
                clock: clock.clone(),
                machine_id: INSTALLATION.parse()?,
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
        while client_observations.borrow().acks.is_empty()
            || client_handoffs.count.load(Ordering::SeqCst) == 0
        {
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
    let mut repair_at = None;
    let mut repaired = false;
    loop {
        let now = sim.elapsed();
        if fault == LinkFault::PartitionBeforePush
            && repair_at.is_none()
            && observed.borrow().connections == 1
        {
            assert_eq!(handoffs.count.load(Ordering::SeqCst), 0);
            assert_eq!(observed.borrow().pushes, 0);
            sim.partition(ROUTER_HOST, CENTRAL_HOST);
            repair_at = Some(now + Duration::from_secs(85));
        }
        if !repaired && repair_at.is_some_and(|repair| now >= repair) {
            assert!(observed.borrow().acks.is_empty());
            sim.repair(ROUTER_HOST, CENTRAL_HOST);
            repaired = true;
        }
        if sim.step()? {
            break;
        }
    }
    let observed = observed.borrow();
    assert!(observed.connections >= 2);
    assert_eq!(handoffs.count.load(Ordering::SeqCst), 1);
    let [(accepted_at, DeliveryAnswer::Accepted)] = observed.acks.as_slice() else {
        return Err(format!("central never received its one Ack: {:?}", observed.acks).into());
    };
    assert_on_minute(*accepted_at);
    match fault {
        LinkFault::PartitionBeforePush => {
            assert!(repaired);
            assert!(observed.pushes >= 2);
            assert!(*accepted_at >= repair_at.ok_or("partition never started")?);
            assert_eq!(observed.discarded_ack_at, None);
        }
        LinkFault::DisconnectBeforeSettlingAck => {
            assert_eq!(observed.pushes, 1);
            let lost_at = observed
                .discarded_ack_at
                .ok_or("the first Ack did not reach the socket")?;
            assert_on_minute(lost_at);
            assert!(*accepted_at >= lost_at + Duration::from_secs(54));
        }
    }
    Ok(())
}

fn assert_on_minute(observed_at: Duration) {
    let remainder = observed_at.as_millis() % 60_000;
    assert!(remainder <= 3_000 || remainder >= 57_000);
}

#[test]
fn a_partition_across_delivery_waits_for_the_reconnect_push() -> turmoil::Result {
    run_delivery(LinkFault::PartitionBeforePush)
}

#[test]
fn reconnect_resends_after_a_lost_ack_without_repeating_session_handoff() -> turmoil::Result {
    run_delivery(LinkFault::DisconnectBeforeSettlingAck)
}
