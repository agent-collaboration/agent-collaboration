//! A real SDK pump carries Query; only remote relay answers stand in.
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_mail_protocol::CarrierAcceptance;
use fleet_protocol::{
    ClientFrame, DeliveryAnswer, DeliveryRequest, ItemId, ItemStatus, ItemStatusState,
    OperationAnswer, OperationRequest, PeerId, QueryResult, SchemaSupport, ServerFrame,
};
use fleet_session::{Endpoint, PeerCredential, SessionClient};
use fleet_session_client::SessionPump;
use futures_util::{SinkExt, StreamExt};
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, watch},
    task::JoinSet,
};

type QueriedItemStatus = (ItemId, ItemStatus<CarrierAcceptance>);

pub(crate) struct FrozenMailQuery {
    answers: Arc<Mutex<Vec<QueriedItemStatus>>>,
    accept_submissions: Arc<AtomicBool>,
    submissions: Arc<Mutex<Vec<fleet_protocol::DeliveryItemRef>>>,
    pub session: collaboration_runtime::MailDeliverySession,
    pub entered: mpsc::UnboundedReceiver<Vec<ItemId>>,
    released: watch::Sender<bool>,
    stopped: watch::Sender<bool>,
    tasks: JoinSet<()>,
}
impl FrozenMailQuery {
    pub async fn open(root: &Path, peer: PeerId) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
        let (released, mut release) = watch::channel(false);
        let (stopped, stop) = watch::channel(false);
        let (entered, observing) = mpsc::unbounded_channel();
        let answers = Arc::new(Mutex::new(
            Vec::<(ItemId, ItemStatus<CarrierAcceptance>)>::new(),
        ));
        let scripted = answers.clone();
        let accept_submissions = Arc::new(AtomicBool::new(false));
        let accepting = accept_submissions.clone();
        let submissions = Arc::new(Mutex::new(Vec::new()));
        let submitted = submissions.clone();
        let mut tasks = JoinSet::new();
        let mut server_stop = stop.clone();
        tasks.spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(stream).await.unwrap();
            loop {
                let frame = tokio::select! {
                    _ = server_stop.changed() => break,
                    frame = socket.next() => frame,
                };
                let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes))) = frame else {
                    break;
                };
                let answer: ServerFrame<BoardConsumerProtocol> = match serde_json::from_slice::<
                    ClientFrame<BoardConsumerProtocol>,
                >(&bytes)
                .unwrap()
                {
                    ClientFrame::Hello { session_id, .. } => ServerFrame::Welcome {
                        capabilities: None,
                        session_id,
                        databases: Vec::new(),
                    },
                    ClientFrame::Request {
                        frame,
                        body:
                            OperationRequest::Consumer(
                                board_sync_protocol::BoardConsumerRequest::MailSubmit {
                                    item, ..
                                },
                            ),
                    } if accepting.load(Ordering::SeqCst) => {
                        submitted
                            .lock()
                            .unwrap()
                            .push(fleet_protocol::DeliveryItemRef {
                                item: item.id(),
                                digest: item.digest().clone(),
                            });
                        // A re-submit of an already scripted canonical item must
                        // preserve that status, just as the real mailbox does.
                        let known = scripted
                            .lock()
                            .unwrap()
                            .iter()
                            .find(|(requested, _)| *requested == item.id())
                            .map(|(_, answer)| answer.state.clone());
                        let admission = match known {
                            Some(ItemStatusState::Known {
                                status,
                                version,
                                cancel,
                                ..
                            }) => fleet_protocol::SubmitResult::Replayed {
                                status,
                                version,
                                cancel,
                            },
                            _ => fleet_protocol::SubmitResult::Accepted {
                                status: fleet_protocol::DeliveryStatus::Held,
                                version: fleet_protocol::StatusVersion::new(1),
                                cancel: Some(fleet_protocol::CancelResult::Pending),
                            },
                        };
                        ServerFrame::Answer {
                            frame,
                            body: OperationAnswer::Consumer(
                                board_sync_protocol::BoardConsumerAnswer::MailSubmit(
                                    board_sync_protocol::MailSubmitAnswer::Submitted(admission),
                                ),
                            ),
                        }
                    }
                    ClientFrame::Request {
                        frame,
                        body: OperationRequest::Delivery(DeliveryRequest::Query { items, .. }),
                    } => {
                        entered.send(items.clone()).unwrap();
                        while !*release.borrow_and_update() {
                            tokio::select! {
                                _ = server_stop.changed() => return,
                                result = release.changed() => result.unwrap(),
                            }
                        }
                        ServerFrame::Answer {
                            frame,
                            body: OperationAnswer::Delivery(DeliveryAnswer::Query(QueryResult {
                                statuses: items
                                    .into_iter()
                                    .map(|item| {
                                        scripted
                                            .lock()
                                            .unwrap()
                                            .iter()
                                            .find(|(requested, _)| *requested == item)
                                            .map(|(_, answer)| answer.clone())
                                            .unwrap_or(ItemStatus {
                                                item,
                                                state: ItemStatusState::Unknown,
                                            })
                                    })
                                    .collect(),
                            })),
                        }
                    }
                    ClientFrame::Request { frame, .. } => ServerFrame::Answer {
                        frame,
                        body: OperationAnswer::InvalidShape {
                            field: "only Query and enabled MailSubmit are scripted".into(),
                        },
                    },
                    ClientFrame::Answer { .. } => continue,
                };
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Binary(
                        serde_json::to_vec(&answer).unwrap().into(),
                    ))
                    .await
                    .unwrap();
            }
        });
        let bound = Duration::from_secs(30);
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            ReqwestSessionTransport::new(
                AccessServiceToken::new("query-fixture", "query-fixture").unwrap(),
                bound,
                bound,
            )
            .unwrap(),
            MachineSessionStore::open(root).await.unwrap(),
            &UuidV7Ids,
            SchemaSupport { max_version: 1 },
            Backoff {
                initial: bound,
                max: bound,
                factor: 2,
            },
        )
        .await
        .unwrap();
        let (requests, mut events, pump) = SessionPump::new(
            client,
            Endpoint::new(&endpoint).unwrap(),
            PeerCredential::new("Bearer query-fixture").unwrap(),
            bound,
        );
        tasks.spawn(async move {
            pump.run(stop).await.unwrap();
        });
        let (welcomed, mut readiness) = watch::channel(false);
        tasks.spawn(async move {
            while let Some(event) = events.recv().await {
                if matches!(event, fleet_session::ClientEvent::Welcome { .. }) {
                    welcomed.send_replace(true);
                }
            }
        });
        tokio::time::timeout(Duration::from_secs(6), async {
            while !*readiness.borrow_and_update() {
                readiness.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        Self {
            answers,
            accept_submissions,
            submissions,
            session: collaboration_runtime::MailDeliverySession {
                requests,
                welcomed: readiness,
                peer,
                relay: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
            },
            entered: observing,
            released,
            stopped,
            tasks,
        }
    }
    /// Script the typed mailbox answer, including deliberate ID/digest corruption.
    pub fn answer_for(&self, requested: ItemId, answer: ItemStatus<CarrierAcceptance>) {
        let mut answers = self.answers.lock().unwrap();
        answers.retain(|(item, _)| *item != requested);
        answers.push((requested, answer));
    }
    pub fn release(&self) {
        self.released.send_replace(true);
    }
    pub fn accept_submissions(&self) {
        self.accept_submissions.store(true, Ordering::SeqCst);
    }
    pub fn submissions(&self) -> Vec<fleet_protocol::DeliveryItemRef> {
        self.submissions.lock().unwrap().clone()
    }
    pub async fn close(mut self) {
        self.release();
        self.stopped.send_replace(true);
        while let Some(task) = self.tasks.join_next().await {
            task.unwrap();
        }
    }
}
