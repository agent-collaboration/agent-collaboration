//! Start and drain the real Host event loop around one actual library pump.
use super::*;
use std::path::PathBuf;
#[path = "installation_directory_refresh.rs"]
mod installation_directory_refresh;
#[path = "notification_directory.rs"]
mod notification_directory;
#[path = "notification_frontier.rs"]
mod notification_frontier;
#[path = "notification_owner.rs"]
mod notification_owner;
#[path = "notification_prefix.rs"]
mod notification_prefix;
#[path = "notification_remote_cancellation.rs"]
mod notification_remote_cancellation;
#[path = "notification_replication.rs"]
mod notification_replication;
#[path = "notification_welcome.rs"]
mod notification_welcome;
#[path = "mail_remote_frontdoor.rs"]
mod remote_frontdoor;
#[path = "notification_split_transport.rs"]
mod split_transport;

pub(super) struct MailHarness {
    pub root: PathBuf,
    pub mail: mail_owner::MailOwner,
    pub welcomed: watch::Sender<bool>,
    pub transport: NoticeTransport,
    pub relay: fleet_sync_protocol::PeerId,
    directory: Option<tempfile::TempDir>,
    central: TestFleet,
    ledger: Arc<NoticeLedger>,
    requests: fleet_sync_tokio_runner::SessionRequests<BoardConsumerProtocol>,
    stop: watch::Sender<bool>,
    tasks: JoinSet<io::Result<()>>,
    projects: Arc<ProjectWriters>,
    registry: Arc<ProjectStoreRegistry>,
    release_welcome: Option<tokio::sync::oneshot::Sender<()>>,
    mail_stop: tokio_util::sync::CancellationToken,
    mail_started: bool,
    manual_events: Option<mpsc::Receiver<ClientEvent<BoardConsumerProtocol>>>,
    event_sender: mpsc::Sender<ClientEvent<BoardConsumerProtocol>>,
    admission: Weak<ProjectAdmission>,
    fleet: Weak<FleetClient>,
    publisher: Weak<collaboration_service::NotificationPublisher>,
}
struct DrainedMailHarness {
    central: TestFleet,
    ledger: Arc<NoticeLedger>,
    directory: Option<tempfile::TempDir>,
}

impl MailHarness {
    pub async fn open(initial_notice: bool) -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        let base = directory.path().to_owned();
        Self::open_configured(&base, Some(directory), initial_notice, false, None, None).await
    }
    pub(super) async fn open_manual(
        base: &Path,
        peer: Option<fleet_sync_protocol::PeerId>,
    ) -> Result<Self, TestError> {
        Self::open_configured(base, None, false, true, peer, None).await
    }
    async fn open_configured(
        base: &Path,
        directory: Option<tempfile::TempDir>,
        initial_notice: bool,
        manual: bool,
        peer: Option<fleet_sync_protocol::PeerId>,
        existing_central: Option<TestFleet>,
    ) -> Result<Self, TestError> {
        let root = base.join("router");
        tokio::fs::create_dir_all(&root).await?;
        let release_authentication = existing_central.is_none();
        let mut central = match existing_central {
            Some(central) => central,
            None => TestFleet::start_mail_as(&base.join("central"), peer).await?,
        };
        let ledger = NoticeLedger::open(&root, central.peer)
            .await
            .map_err(|error| io::Error::other(format!("fixture notice-ledger open: {error:?}")))?;
        let inputs = central.inputs()?;
        let transport = NoticeTransport::new(
            ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?,
            ledger.clone(),
            initial_notice,
        );
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            transport.clone(),
            MachineSessionStore::open(&root).await?,
            &UuidV7Ids,
            SchemaSupport { max_version: 1 },
            Backoff {
                initial: Duration::from_secs(1),
                max: BOUND,
                factor: 2,
            },
        )
        .await?;
        let (requests, mut actual_events, pump) =
            SessionPump::new(client, inputs.endpoint, inputs.credential, BOUND);
        central.install_delivery_requests(requests.clone()).await?;
        let (welcomed, readiness) = watch::channel(false);
        let (stop, stopped) = watch::channel(false);
        let mail = mail_owner::MailOwner::open(
            &root,
            central.peer,
            collaboration_runtime::MailDeliverySession {
                requests: requests.clone(),
                welcomed: readiness.clone(),
                peer: central.peer,
                relay: inputs.relay,
            },
        )
        .await?;
        let registry = Arc::new(ProjectStoreRegistry::new(
            &root,
            Some(central.peer),
            Arc::new(runtime_primitives::SystemClock),
        ));
        let projects = Arc::new(ProjectWriters::new(
            &root,
            central.peer,
            &registry,
            requests.clone(),
            inputs.receiver,
            readiness,
            stopped.clone(),
        ));
        projects.prepare_existing().await?;
        let (events, receiving) = mpsc::channel(128);
        let event_sender = events.clone();
        let (welcome_seen, welcome_observed) = tokio::sync::oneshot::channel();
        let (release_welcome, welcome_released) = tokio::sync::oneshot::channel();
        let mut tasks = JoinSet::new();
        tasks.spawn(async move { pump.run(stopped).await.map_err(io::Error::other) });
        let mut forward_stop = stop.subscribe();
        tasks.spawn(async move {
            let mut gate = Some((welcome_seen, welcome_released));
            while let Some(event) = actual_events.recv().await {
                if matches!(event, ClientEvent::Welcome { .. })
                    && let Some((seen, released)) = gate.take()
                {
                    seen.send(())
                        .map_err(|_| io::Error::other("Welcome observer closed"))?;
                    tokio::select! {
                        result = released => result.map_err(io::Error::other)?,
                        _ = forward_stop.changed() => return Ok(()),
                    }
                }
                if events.send(event).await.is_err() {
                    break;
                }
            }
            Ok(())
        });
        let manual_events = if manual {
            Some(receiving)
        } else {
            tasks.spawn(run_events(
                receiving,
                requests.clone(),
                MachineNoticeReactions::open(&root).await?,
                MachineSessionEventOwners {
                    #[cfg(feature = "runtime-test-support")]
                    progress: Default::default(),
                    projects: projects.clone(),
                    admission: Weak::new(),
                    fleet: Weak::new(),
                    publisher: Weak::new(),
                    mail: Arc::downgrade(&mail.mail),
                },
                welcomed.clone(),
                stop.subscribe(),
            ));
            None
        };
        tokio::time::timeout(BOUND, central.auth.entered.notified()).await?;
        if release_authentication {
            central.release()?;
        }
        tokio::time::timeout(BOUND, welcome_observed).await??;

        Ok(Self {
            root,
            mail,
            welcomed,
            directory,
            central,
            ledger,
            requests,
            stop,
            tasks,
            projects,
            registry,
            release_welcome: Some(release_welcome),
            transport,
            relay: inputs.relay,
            mail_stop: tokio_util::sync::CancellationToken::new(),
            mail_started: false,
            manual_events,
            event_sender,
            admission: Weak::new(),
            fleet: Weak::new(),
            publisher: Weak::new(),
        })
    }
    pub(super) async fn next_manual_event(
        &mut self,
    ) -> Result<ClientEvent<BoardConsumerProtocol>, TestError> {
        tokio::time::timeout(
            BOUND,
            self.manual_events
                .as_mut()
                .ok_or("automatic event owner already started")?
                .recv(),
        )
        .await?
        .ok_or_else(|| io::Error::other("actual session event stream closed").into())
    }
    async fn resume_events(
        &mut self,
        initial: Vec<ClientEvent<BoardConsumerProtocol>>,
    ) -> Result<(), TestError> {
        let receiving = self
            .manual_events
            .take()
            .ok_or("automatic event owner already started")?;
        for event in initial {
            self.event_sender.send(event).await?;
        }
        self.tasks.spawn(run_events(
            receiving,
            self.requests.clone(),
            MachineNoticeReactions::open(&self.root).await?,
            MachineSessionEventOwners {
                #[cfg(feature = "runtime-test-support")]
                progress: Default::default(),
                projects: self.projects.clone(),
                admission: self.admission.clone(),
                fleet: self.fleet.clone(),
                publisher: self.publisher.clone(),
                mail: Arc::downgrade(&self.mail.mail),
            },
            self.welcomed.clone(),
            self.stop.subscribe(),
        ));
        Ok(())
    }
    pub fn release_welcome(&mut self) -> Result<(), TestError> {
        self.release_welcome
            .take()
            .ok_or("Welcome already released")?
            .send(())
            .map_err(|_| io::Error::other("Welcome gate closed"))?;
        Ok(())
    }
    pub fn peer(&self) -> fleet_sync_protocol::PeerId {
        self.central.peer
    }
    pub async fn establish_epoch_two(
        &self,
        project: &message_board::ProjectId,
    ) -> Result<super::super::super::fleet_service::EpochTransfer, TestError> {
        self.central
            .establish_epoch_two(&self.root.join("successor-session"), project)
            .await
    }
    pub async fn move_to_epoch_three(
        &self,
        project: &message_board::ProjectId,
        transfer: &super::super::super::fleet_service::EpochTransfer,
    ) -> Result<(), TestError> {
        self.central.move_to_epoch_three(project, transfer).await
    }
    pub fn connections(&self) -> usize {
        self.central.auth.upgrades.load(Ordering::SeqCst)
    }
    pub fn start_mail(&mut self) {
        assert!(
            !self.mail_started,
            "one mail engine runner per physical owner"
        );
        self.mail_started = true;
        let mail = self.mail.mail.clone();
        let stop = self.mail_stop.clone();
        self.tasks
            .spawn(async move { mail.run(stop).await.map_err(io::Error::other) });
    }
    pub async fn submit_generic(
        &self,
        item: fleet_sync_protocol::DeliveryEnvelope<board_sync_protocol::BoardDeliveryTypes>,
    ) -> Result<
        fleet_sync_protocol::SubmitResult<collaboration_mail_protocol::CarrierAcceptance>,
        TestError,
    > {
        let call = runtime_primitives::CallId {
            operation: runtime_primitives::OperationId::new(durable_steps::Uuid::now_v7())?,
            step: runtime_primitives::StepName::new("fixture-submit")?,
        };
        match self
            .requests
            .request(OperationRequest::Delivery(
                fleet_sync_protocol::DeliveryRequest::Submit { call, item },
            ))
            .await?
        {
            OperationAnswer::Delivery(fleet_sync_protocol::DeliveryAnswer::Submit(answer)) => {
                Ok(answer)
            }
            _ => Err(io::Error::other("real relay returned a different submit answer").into()),
        }
    }
    pub async fn query(
        &self,
        items: Vec<fleet_sync_protocol::ItemId>,
    ) -> Result<
        fleet_sync_protocol::QueryResult<collaboration_mail_protocol::CarrierAcceptance>,
        TestError,
    > {
        let call = runtime_primitives::CallId {
            operation: runtime_primitives::OperationId::new(durable_steps::Uuid::now_v7())?,
            step: runtime_primitives::StepName::new("fixture-query")?,
        };
        match self
            .requests
            .request(OperationRequest::Delivery(
                fleet_sync_protocol::DeliveryRequest::Query {
                    call,
                    mailbox: self.peer(),
                    items,
                },
            ))
            .await?
        {
            OperationAnswer::Delivery(fleet_sync_protocol::DeliveryAnswer::Query(answer)) => {
                Ok(answer)
            }
            _ => Err(io::Error::other("real relay returned a different query answer").into()),
        }
    }
    pub async fn status_page(
        &self,
        after: u64,
        limit: u32,
    ) -> Result<
        fleet_sync_protocol::StatusFetchResult<collaboration_mail_protocol::CarrierAcceptance>,
        TestError,
    > {
        let call = runtime_primitives::CallId {
            operation: runtime_primitives::OperationId::new(durable_steps::Uuid::now_v7())?,
            step: runtime_primitives::StepName::new("fixture-status-readiness")?,
        };
        match self
            .requests
            .request(OperationRequest::Delivery(
                fleet_sync_protocol::DeliveryRequest::StatusFetch {
                    call,
                    after: fleet_sync_protocol::StatusCursor::new(after),
                    limit,
                },
            ))
            .await?
        {
            OperationAnswer::Delivery(fleet_sync_protocol::DeliveryAnswer::StatusFetch(batch)) => {
                Ok(batch)
            }
            _ => Err(io::Error::other("real relay returned a different status answer").into()),
        }
    }
    pub async fn ack(&self) -> Result<NoticePosition, TestError> {
        Ok(MachineSessionStore::open(&self.root)
            .await?
            .load()
            .await?
            .ok_or("resume missing")?
            .notice_cursor)
    }
    pub async fn wait_state(
        &self,
        ready: impl Fn(&serde_json::Value) -> bool,
        failure: &'static str,
    ) -> Result<serde_json::Value, TestError> {
        let started = std::time::Instant::now();
        tokio::time::timeout(BOUND, async {
            loop {
                if let Some(state) = self.mail.state().await?
                    && ready(&state)
                {
                    return Ok(state);
                }
                if started.elapsed() >= BOUND {
                    return Err(io::Error::other(failure).into());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| io::Error::other(failure))?
    }
    pub async fn wait_ack(&self, position: NoticePosition) -> Result<(), TestError> {
        tokio::time::timeout(BOUND, async {
            loop {
                if self.ack().await? == position {
                    return Ok(());
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| io::Error::other("durable client acknowledgement did not advance"))?
    }
    pub async fn close(self) -> Result<(), TestError> {
        let DrainedMailHarness {
            mut central,
            ledger,
            directory,
        } = self.drain_local().await?;
        central
            .stop()
            .await
            .map_err(|error| io::Error::other(format!("mail harness central shutdown: {error}")))?;
        Arc::try_unwrap(ledger)
            .map_err(|_| io::Error::other("notice source retained"))?
            .close()
            .await?;
        drop(directory);
        Ok(())
    }
    pub(super) async fn close_keeping_central(self) -> Result<TestFleet, TestError> {
        let DrainedMailHarness {
            central,
            ledger,
            directory,
        } = self.drain_local().await?;
        // Only the drained client is detached; the next owner must install its
        // own actual request handle, and concurrent installations still refuse.
        central.remove_delivery_requests().await;
        Arc::try_unwrap(ledger)
            .map_err(|_| io::Error::other("notice source retained"))?
            .close()
            .await?;
        drop(directory);
        Ok(central)
    }
    async fn drain_local(mut self) -> Result<DrainedMailHarness, TestError> {
        self.stop.send_replace(true);
        self.mail_stop.cancel();
        while let Some(result) = tokio::time::timeout(BOUND, self.tasks.join_next())
            .await
            .map_err(|error| {
                io::Error::other(format!("mail harness pump/event/runner drain: {error}"))
            })?
        {
            result??;
        }
        self.projects.shutdown().await?;
        let Self {
            projects,
            registry,
            requests,
            mail,
            central,
            ledger,
            directory,
            transport,
            event_sender,
            ..
        } = self;
        drop(projects);
        drop(registry);
        drop(requests);
        drop(transport);
        drop(event_sender);
        mail.close().await?;
        Ok(DrainedMailHarness {
            central,
            ledger,
            directory,
        })
    }
}

#[path = "mail_remote_crash.rs"]
mod crash;
#[path = "mail_remote_restart.rs"]
mod restart;

#[path = "notice_reaction_dispatch.rs"]
mod reaction_dispatch;
