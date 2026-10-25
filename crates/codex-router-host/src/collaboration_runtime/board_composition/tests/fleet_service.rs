//! Real generic FleetService; directory policy and enrollment are explicit fixture inputs.
use board_fleet_sync::{AccessServiceToken, BOARD_LEASE_POLICY, BoardSessionInputs};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardReadCursor,
    BoardReadPage, BoardReadScope, MailSubmitAnswer, NoticeRead,
};
use durable_steps::CallId;
use fleet_sync_lease::{GateFailure, HolderChangeGate, LifecycleGate, RegistryConfig};
use fleet_sync_protocol::{
    AuthRefusal, DatabaseId, DeliveryAnswer, DeliveryRequest, GateAnswer, HolderChangeRequest,
    OperationAnswer, OperationRequest, PeerId, ReceiverId, SettleOutcome,
};
use fleet_sync_receiver::{
    AppliedPiece, FollowUpDone, FollowUpFailure, FollowUpHandler, HookError, ReceiverRules,
};
use fleet_sync_service::{
    ConsumerHandler, FleetService, ServiceConsumer, ServiceError, ServiceOptions, ServiceScope,
};
use fleet_sync_session::{Endpoint, PeerAuthenticator, PeerCredential, VerifiedPeer};
use fleet_sync_tokio_runner::SessionRequests;
use runtime_primitives::{Clock, SystemClock};
use std::{
    collections::VecDeque,
    io,
    net::SocketAddr,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};

pub(super) const BOUND: Duration = Duration::from_secs(30);

#[path = "mail_epoch_transfer.rs"]
mod epoch_transfer;
pub(super) use epoch_transfer::EpochTransfer;
#[path = "notification_probe.rs"]
mod notification_probe;
use notification_probe::NotificationProbe;
#[path = "notification_directory_probe.rs"]
mod notification_directory_probe;
use notification_directory_probe::DirectoryProbe;

#[path = "notification_fleet_probe.rs"]
mod fleet_read_probe;
use fleet_read_probe::FleetReadProbe;

struct Consumer {
    directory: Arc<DirectoryProbe>,
    fleet_reads: Arc<FleetReadProbe>,
    read_copy_entries: Arc<Mutex<Vec<fleet_admission::DirectoryEntry>>>,
    read_copy_unavailable: Arc<AtomicBool>,
    read_copy_requests: Arc<AtomicUsize>,
    notifications: Arc<NotificationProbe>,
    registration: GateAnswer,
    registration_gate: Arc<RegistrationGate>,
    notices: Arc<NoticeScripts>,
    delivery_requests: Arc<Mutex<Option<SessionRequests<BoardConsumerProtocol>>>>,
}

#[derive(Default)]
struct NoticeScriptState {
    answers: VecDeque<NoticeRead>,
    after: Vec<Option<fleet_sync_protocol::NoticePosition>>,
}
#[derive(Default)]
struct NoticeScripts {
    state: Mutex<NoticeScriptState>,
    read: Notify,
}

pub(super) struct RegistrationGate {
    pub entered: Notify,
    held: Mutex<Option<oneshot::Receiver<()>>>,
}
impl ConsumerHandler<BoardConsumerProtocol> for Consumer {
    async fn request(
        &self,
        _: PeerId,
        request: BoardConsumerRequest,
    ) -> Result<BoardConsumerAnswer, ServiceError> {
        if matches!(
            request,
            BoardConsumerRequest::Read {
                scope: BoardReadScope::Directory { .. },
                ..
            }
        ) {
            self.read_copy_requests.fetch_add(1, Ordering::SeqCst);
            if self.read_copy_unavailable.load(Ordering::SeqCst) {
                return Err(ServiceError::Rule {
                    message: "fixture directory temporarily unavailable".into(),
                });
            }
            // Consumer policy is a fixture input; SDK snapshot transport and
            // installation still run through this actual FleetService.
            let entries = self
                .read_copy_entries
                .lock()
                .await
                .iter()
                .map(board_sync_protocol::BoardPayload::encode)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| ServiceError::Rule {
                    message: error.to_string(),
                })?;
            return Ok(BoardConsumerAnswer::Read(BoardReadPage::Directory {
                entries,
                as_of: chrono::Utc::now(),
            }));
        }
        if let BoardConsumerRequest::Read {
            scope: BoardReadScope::InstallationPeers { after },
            cursor: None,
        } = request
        {
            return Ok(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeers(
                self.directory.page(after).await?,
            )));
        }
        if let BoardConsumerRequest::Read {
            scope: BoardReadScope::InstallationPeer { ref service },
            cursor: None,
        } = request
        {
            return Ok(BoardConsumerAnswer::Read(BoardReadPage::InstallationPeer(
                self.directory.lookup(service).await?,
            )));
        }
        if let BoardConsumerRequest::Read {
            scope: BoardReadScope::Notices,
            cursor,
        } = request
        {
            let after = match cursor {
                None => None,
                Some(BoardReadCursor::Notices(after)) => Some(after),
                _ => {
                    return Err(ServiceError::Rule {
                        message: "invalid fixture notice cursor".to_owned(),
                    });
                }
            };
            let mut state = self.notices.state.lock().await;
            let answer = state
                .answers
                .pop_front()
                .unwrap_or_else(|| NoticeRead::Page {
                    notices: Vec::new(),
                    as_of: chrono::Utc::now(),
                });
            state.after.push(after);
            drop(state);
            self.notices.read.notify_waiters();
            return Ok(BoardConsumerAnswer::Read(BoardReadPage::Notices(answer)));
        }
        if let BoardConsumerRequest::MailSubmit { item, origin_epoch } = request {
            self.notifications
                .record_submission(&item, origin_epoch)
                .await
                .map_err(|error| ServiceError::Rule {
                    message: error.to_string(),
                })?;
            if origin_epoch.is_some() {
                // Project authority is B/G3's real consumer policy, outside this stand-in.
                return Ok(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::NotWriter));
            }
            let requests = self
                .delivery_requests
                .lock()
                .await
                .as_ref()
                .cloned()
                .ok_or_else(|| ServiceError::Rule {
                    message: "fixture delivery client is not installed".to_owned(),
                })?;
            let call = CallId {
                operation: runtime_primitives::OperationId::new(item.id().as_uuid()).map_err(
                    |error| ServiceError::Rule {
                        message: error.to_string(),
                    },
                )?,
                step: runtime_primitives::StepName::new("fixture-mail-submit").map_err(
                    |error| ServiceError::Rule {
                        message: error.to_string(),
                    },
                )?,
            };
            // Decision 692: only Board policy stands in. The same actual session
            // makes the generic call; FleetService owns custody and its answer.
            return match requests
                .request(OperationRequest::Delivery(DeliveryRequest::Submit {
                    call,
                    item: *item,
                }))
                .await
            {
                Ok(OperationAnswer::Delivery(DeliveryAnswer::Submit(answer))) => Ok(
                    BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(answer)),
                ),
                _ => Err(ServiceError::Rule {
                    message: "fixture relay submit unavailable".to_owned(),
                }),
            };
        }
        if let BoardConsumerRequest::Read { scope, cursor } = request
            && matches!(
                scope,
                BoardReadScope::FleetRecords | BoardReadScope::FleetSnapshot
            )
        {
            // Board policy remains explicit fixture input, separate from the
            // actual generic registry/receiver and native writer under test.
            return Ok(BoardConsumerAnswer::Read(
                self.fleet_reads.read(&scope, cursor).await?,
            ));
        }
        Err(ServiceError::Rule {
            message: "board policy is outside the generic G2 fixture".to_owned(),
        })
    }
}
impl ServiceConsumer<BoardConsumerProtocol> for Consumer {
    type HookItem = ();
}
impl ReceiverRules<ServiceScope<'_>> for Consumer {
    type Protocol = BoardConsumerProtocol;
    type FollowUp = ();
    async fn on_applied(
        &self,
        _: &mut ServiceScope<'_>,
        piece: &AppliedPiece<'_, board_sync_protocol::BoardAnnotation>,
    ) -> Result<Vec<()>, HookError> {
        self.notifications.applied(piece).await?;
        Ok(Vec::new())
    }
}

impl FollowUpHandler for Consumer {
    type Item = ();

    async fn follow_up(&self, _: &(), _: CallId) -> Result<FollowUpDone, FollowUpFailure> {
        Ok(FollowUpDone)
    }
}
impl LifecycleGate for Consumer {
    async fn admit_registration(
        &self,
        _: &DatabaseId,
        _: &PeerId,
        _: CallId,
    ) -> Result<GateAnswer, GateFailure> {
        self.registration_gate.entered.notify_one();
        if let Some(held) = self.registration_gate.held.lock().await.take() {
            held.await.map_err(|_| GateFailure {
                detail: "fixture registration gate closed".to_owned(),
            })?;
        }
        Ok(self.registration.clone())
    }
    async fn admit_retirement(&self, _: &DatabaseId, _: CallId) -> Result<GateAnswer, GateFailure> {
        Ok(GateAnswer::Admitted)
    }
}
impl HolderChangeGate for Consumer {
    async fn reserve(&self, _: &HolderChangeRequest, _: CallId) -> Result<GateAnswer, GateFailure> {
        Ok(GateAnswer::Admitted)
    }
    async fn settle(
        &self,
        _: &HolderChangeRequest,
        _: SettleOutcome,
        _: CallId,
    ) -> Result<(), GateFailure> {
        Ok(())
    }
}

pub(super) struct Authenticator {
    peer: PeerId,
    successor_peer: PeerId,
    installation: message_board::ServiceId,
    pub entered: Notify,
    pub upgrades: AtomicUsize,
    held: Mutex<Option<oneshot::Receiver<()>>>,
}
impl PeerAuthenticator for Authenticator {
    type Material = String;
    type Route = message_board::ServiceId;

    async fn verify(&self, material: String) -> Result<(VerifiedPeer, Self::Route), AuthRefusal> {
        let (peer, route) = match material.as_str() {
            "Bearer board-session-fixture" => (self.peer, self.installation.clone()),
            "Bearer board-epoch-successor-fixture" => (
                self.successor_peer,
                "00000000-0000-4000-8000-000000000002"
                    .to_owned()
                    .try_into()
                    .map_err(|_| AuthRefusal::Unknown)?,
            ),
            _ => return Err(AuthRefusal::Unknown),
        };
        self.upgrades.fetch_add(1, Ordering::SeqCst);
        self.entered.notify_one();
        if let Some(held) = self.held.lock().await.take() {
            held.await.map_err(|_| AuthRefusal::Unknown)?;
        }
        Ok((
            VerifiedPeer {
                peer,
                expires_at: SystemClock.now().after(Duration::from_secs(3600)),
            },
            route,
        ))
    }
}

pub(super) struct TestFleet {
    pub directory: Arc<DirectoryProbe>,
    #[allow(dead_code)] // Read-copy observations run with runtime-test-support.
    pub read_copy_entries: Arc<Mutex<Vec<fleet_admission::DirectoryEntry>>>,
    #[allow(dead_code)] // Read-copy observations run with runtime-test-support.
    pub read_copy_unavailable: Arc<AtomicBool>,
    #[allow(dead_code)] // Read-copy observations run with runtime-test-support.
    pub read_copy_requests: Arc<AtomicUsize>,
    pub notifications: Arc<NotificationProbe>,
    pub peer: PeerId,
    pub receiver: ReceiverId,
    pub auth: Arc<Authenticator>,
    pub registration_gate: Arc<RegistrationGate>,
    registration_release: Option<oneshot::Sender<()>>,
    address: SocketAddr,
    release: Option<oneshot::Sender<()>>,
    stop: watch::Sender<bool>,
    task: Option<JoinHandle<Result<(), ServiceError>>>,
    notices: Arc<NoticeScripts>,
    fleet_reads: Arc<FleetReadProbe>,
    delivery_requests: Arc<Mutex<Option<SessionRequests<BoardConsumerProtocol>>>>,
    handle: fleet_sync_service::ServiceHandle<BoardConsumerProtocol, Consumer, Authenticator>,
}
impl TestFleet {
    pub async fn start(
        directory: &Path,
        registration: GateAnswer,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::start_configured(directory, registration, None, None, None).await
    }
    pub async fn start_mail_as(
        directory: &Path,
        peer: Option<PeerId>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        Self::start_mail_with_receiver(directory, peer, None).await
    }
    pub async fn start_mail_with_receiver(
        directory: &Path,
        peer: Option<PeerId>,
        receiver: Option<ReceiverId>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use fleet_sync_delivery::{DeliveryLimits, minimum_settlement_allowance};
        let limits = DeliveryLimits::new::<board_sync_protocol::BoardDeliveryTypes>(
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            10_000,
            64 * 1024 * 1024,
            minimum_settlement_allowance::<board_sync_protocol::BoardDeliveryTypes>()?,
        )?;
        Self::start_configured(
            directory,
            GateAnswer::Admitted,
            Some(limits),
            peer,
            receiver,
        )
        .await
    }
    async fn start_configured(
        directory: &Path,
        registration: GateAnswer,
        delivery: Option<fleet_sync_delivery::DeliveryLimits>,
        peer: Option<PeerId>,
        receiver: Option<ReceiverId>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let peer = peer.map_or_else(|| PeerId::new(durable_steps::Uuid::now_v7()), Ok)?;
        let receiver =
            receiver.map_or_else(|| ReceiverId::new(durable_steps::Uuid::now_v7()), Ok)?;
        let (release, held) = oneshot::channel();
        let auth = Arc::new(Authenticator {
            peer,
            successor_peer: PeerId::new(durable_steps::Uuid::now_v7())?,
            installation: "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()?,
            entered: Notify::new(),
            upgrades: AtomicUsize::new(0),
            held: Mutex::new(Some(held)),
        });
        let registration_gate = Arc::new(RegistrationGate {
            entered: Notify::new(),
            held: Mutex::new(None),
        });
        let notices = Arc::new(NoticeScripts::default());
        let fleet_reads = Arc::new(FleetReadProbe::default());
        let delivery_requests = Arc::new(Mutex::new(None));
        let notifications = Arc::new(NotificationProbe::default());
        let directory_probe = Arc::new(DirectoryProbe::default());
        let read_copy_entries = Arc::new(Mutex::new(Vec::new()));
        let read_copy_unavailable = Arc::new(AtomicBool::new(false));
        let read_copy_requests = Arc::new(AtomicUsize::new(0));
        let service = FleetService::<BoardConsumerProtocol, _, _>::open(
            ServiceOptions {
                directory: directory.to_owned(),
                delivery,
                receiver,
                registry: RegistryConfig {
                    lease: BOARD_LEASE_POLICY,
                    authoritative_receiver: receiver,
                    release_unsent_for: Duration::from_secs(5),
                    move_needs_renewal_within: None,
                },
                call_timeout: BOUND,
                socket_timeout: BOUND,
            },
            Arc::new(Consumer {
                directory: directory_probe.clone(),
                fleet_reads: fleet_reads.clone(),
                read_copy_entries: read_copy_entries.clone(),
                read_copy_unavailable: read_copy_unavailable.clone(),
                read_copy_requests: read_copy_requests.clone(),
                notifications: notifications.clone(),
                registration,
                registration_gate: registration_gate.clone(),
                notices: notices.clone(),
                delivery_requests: delivery_requests.clone(),
            }),
            auth.clone(),
        )
        .await?;
        let handle = service.handle();
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(service.serve(listener, axum::Router::new(), stopped));
        Ok(Self {
            directory: directory_probe,
            read_copy_entries,
            read_copy_unavailable,
            read_copy_requests,
            notifications,
            peer,
            receiver,
            auth,
            registration_gate,
            registration_release: None,
            address,
            release: Some(release),
            stop,
            task: Some(task),
            notices,
            fleet_reads,
            delivery_requests,
            handle,
        })
    }
    pub async fn freeze_fleet_snapshot(
        &self,
        snapshot: message_board_storage::FleetRecordSnapshot,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.fleet_reads.freeze(snapshot).await
    }
    pub async fn install_delivery_requests(
        &self,
        requests: SessionRequests<BoardConsumerProtocol>,
    ) -> io::Result<()> {
        let mut installed = self.delivery_requests.lock().await;
        if installed.is_some() {
            return Err(io::Error::other(
                "fixture delivery client already installed",
            ));
        }
        *installed = Some(requests);
        Ok(())
    }
    pub async fn remove_delivery_requests(&self) {
        self.delivery_requests.lock().await.take();
    }
    pub async fn script_notice_answers(&self, answers: Vec<NoticeRead>) {
        let mut state = self.notices.state.lock().await;
        state.answers = answers.into();
        state.after.clear();
    }
    pub async fn notice_reads(&self) -> Vec<Option<fleet_sync_protocol::NoticePosition>> {
        self.notices.state.lock().await.after.clone()
    }
    pub async fn wait_notice_reads(
        &self,
        count: usize,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        timeout(BOUND, async {
            loop {
                let changed = self.notices.read.notified();
                if self.notices.state.lock().await.after.len() >= count {
                    return;
                }
                changed.await;
            }
        })
        .await?;
        Ok(())
    }
    pub fn inputs(&self) -> Result<BoardSessionInputs, Box<dyn std::error::Error + Send + Sync>> {
        Ok(BoardSessionInputs {
            endpoint: Endpoint::new(&format!("ws://{}/session", self.address))?,
            access: AccessServiceToken::new("fixture-access-id", "fixture-access-secret")?,
            credential: PeerCredential::new("Bearer board-session-fixture")?,
            peer: self.peer,
            receiver: self.receiver,
            // Fixture enrollment supplies a deployment relay independently of this peer.
            relay: "0192a1b2-c3d4-7000-8000-000000000003".parse()?,
        })
    }
    pub fn recipient_inputs(
        &self,
    ) -> Result<BoardSessionInputs, Box<dyn std::error::Error + Send + Sync>> {
        let mut inputs = self.inputs()?;
        inputs.peer = self.auth.successor_peer;
        inputs.credential = PeerCredential::new("Bearer board-epoch-successor-fixture")?;
        Ok(inputs)
    }
    pub fn release(&mut self) -> io::Result<()> {
        self.release
            .take()
            .ok_or_else(|| io::Error::other("authentication already released"))?
            .send(())
            .map_err(|_| io::Error::other("authentication waiter disappeared"))
    }
    pub async fn hold_registration(&mut self) -> io::Result<()> {
        if self.registration_release.is_some() {
            return Err(io::Error::other("fixture registration is already held"));
        }
        let (release, held) = oneshot::channel();
        *self.registration_gate.held.lock().await = Some(held);
        self.registration_release = Some(release);
        Ok(())
    }
    pub fn release_registration(&mut self) -> io::Result<()> {
        self.registration_release
            .take()
            .ok_or_else(|| io::Error::other("fixture registration is not held"))?
            .send(())
            .map_err(|_| io::Error::other("fixture registration waiter disappeared"))
    }
    pub async fn stop(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.stop.send_replace(true);
        if let Some(task) = self.task.take() {
            timeout(BOUND, task).await.map_err(|error| {
                io::Error::other(format!("SDK central serve task drain: {error}"))
            })???;
        }
        Ok(())
    }
}
impl Drop for TestFleet {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
