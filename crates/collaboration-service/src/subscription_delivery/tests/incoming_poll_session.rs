//! A real SDK pump carries Fetch/Received/Report; the remote mailbox answers stand in.
use board_fleet_sync::{AccessServiceToken, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use collaboration_mail_protocol::CarrierAcceptance;
use fleet_sync_protocol::*;
use fleet_sync_session::{Endpoint, PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use futures_util::{SinkExt, StreamExt};
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::watch, task::JoinSet};

type ReportedOutcome = (ItemId, RecipientOutcome<CarrierAcceptance>);

pub(super) struct IncomingPollSession {
    pub session: crate::MailDeliverySession,
    pub reports: Arc<Mutex<Vec<ReportedOutcome>>>,
    stop: watch::Sender<bool>,
    tasks: JoinSet<()>,
}

impl IncomingPollSession {
    pub async fn open(root: &Path, peer: PeerId, item: DeliveryItem<BoardDeliveryTypes>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/session", listener.local_addr().unwrap());
        let wire = serde_json::to_value(DeliveryEnvelope::Item(item)).unwrap();
        let reports = Arc::new(Mutex::new(Vec::new()));
        let recorded = reports.clone();
        let (stop, mut server_stop) = watch::channel(false);
        let mut tasks = JoinSet::new();
        tasks.spawn(async move {
            let (stream,_)=listener.accept().await.unwrap();
            let mut socket=tokio_tungstenite::accept_async(stream).await.unwrap();
            loop {
                let next=tokio::select!{_ = server_stop.changed()=>break,next=socket.next()=>next};
                let Some(Ok(tokio_tungstenite::tungstenite::Message::Binary(bytes)))=next else{break;};
                let answer:ServerFrame<BoardConsumerProtocol>=match serde_json::from_slice::<ClientFrame<BoardConsumerProtocol>>(&bytes).unwrap(){
                    ClientFrame::Hello{session_id,..}=>ServerFrame::Welcome{session_id,databases:Vec::new()},
                    ClientFrame::Request{frame,body:OperationRequest::Delivery(DeliveryRequest::Fetch{stream,after,..})}=>{
                        let entries=if stream==DeliveryStream::Items&&after.value()==0 {
                            vec![FetchEntry::Item{seq:DeliverySeq::new(1),item:serde_json::from_value(wire.clone()).unwrap()}]
                        }else{Vec::new()};
                        ServerFrame::Answer{frame,body:OperationAnswer::Delivery(DeliveryAnswer::Fetch(FetchResult{
                            stream,entries,through:DeliverySeq::new(if stream==DeliveryStream::Items{1}else{0}),
                        }))}
                    },
                    ClientFrame::Request{frame,body:OperationRequest::Delivery(DeliveryRequest::Received{stream,through,..})}=>
                        ServerFrame::Answer{frame,body:OperationAnswer::Delivery(DeliveryAnswer::Received(ReceivedResult{stream,released_through:through}))},
                    ClientFrame::Request{frame,body:OperationRequest::Delivery(DeliveryRequest::Report{report,..})}=>{
                        let status=match &report.outcome {
                            RecipientOutcome::Held=>DeliveryStatus::Held,
                            RecipientOutcome::Delivered{acceptance}=>DeliveryStatus::Delivered{acceptance:acceptance.clone()},
                            outcome=>panic!("unexpected Poll report: {outcome:?}"),
                        };
                        recorded.lock().unwrap().push((report.item,report.outcome.clone()));
                        ServerFrame::Answer{frame,body:OperationAnswer::Delivery(DeliveryAnswer::Report(ReportResult::Applied{
                            item:report.item,acknowledged:report.report_seq,status,version:StatusVersion::new(report.report_seq.value()),
                        }))}
                    },
                    ClientFrame::Request{frame,..}=>ServerFrame::Answer{frame,body:OperationAnswer::InvalidShape{field:"only incoming Poll mailbox operations are scripted".into()}},
                    ClientFrame::Answer{..}=>continue,
                };
                socket.send(tokio_tungstenite::tungstenite::Message::Binary(serde_json::to_vec(&answer).unwrap().into())).await.unwrap();
            }
        });
        let bound = Duration::from_secs(6);
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            ReqwestSessionTransport::new(
                AccessServiceToken::new("incoming-poll", "incoming-poll").unwrap(),
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
            PeerCredential::new("Bearer incoming-poll").unwrap(),
            bound,
        );
        let stopped = stop.subscribe();
        tasks.spawn(async move {
            pump.run(stopped).await.unwrap();
        });
        let (welcomed, mut readiness) = watch::channel(false);
        tasks.spawn(async move {
            while let Some(event) = events.recv().await {
                if matches!(event, fleet_sync_session::ClientEvent::Welcome { .. }) {
                    welcomed.send_replace(true);
                }
            }
        });
        tokio::time::timeout(bound, async {
            while !*readiness.borrow_and_update() {
                readiness.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        Self {
            session: crate::MailDeliverySession {
                requests,
                welcomed: readiness,
                peer,
                relay: PeerId::new(uuid::Uuid::now_v7()).unwrap(),
            },
            reports,
            stop,
            tasks,
        }
    }
    pub async fn close(mut self) {
        self.stop.send_replace(true);
        while let Some(task) = self.tasks.join_next().await {
            task.unwrap();
        }
    }
}
