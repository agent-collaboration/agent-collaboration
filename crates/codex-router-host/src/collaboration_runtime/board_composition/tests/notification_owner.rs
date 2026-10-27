//! Compose one actual Host project, publisher, native mail owner and saved session.
use super::*;
use crate::collaboration_runtime::board_composition::tests::notification_publisher::project_fixture::{seed_unacknowledged_notification, seed_unacknowledged_local_notification, NotificationSeed};
use collaboration_service::{CollaborationApplication, MachineIdentity, NotificationPublisher, ProjectAdmissionProps, ApplicationFixture, SessionDeliveryRouter};
use message_board::*;
#[path = "notification_authority_recovery.rs"]
mod authority_recovery;
#[path = "notification_public_controls.rs"]
mod public_controls;

pub(super) struct ProjectNotificationFixture {
    pub(super) harness: MailHarness,
    pub(super) admission: Arc<ProjectAdmission>,
    fleet: Arc<FleetClient>,
    pub(super) publisher: Arc<NotificationPublisher>,
    application: CollaborationApplication,
    pub(super) project: ProjectId,
    root: ThreadId,
    pub(super) seed_item: fleet_sync_protocol::ItemId,
    pub(super) seed_epoch: EpochNumber,
}
impl ProjectNotificationFixture {
    pub(super) async fn open_local(base: &Path) -> Result<Self, TestError> {
        let mut central = TestFleet::start_mail_as(&base.join("central"), None).await?;
        let seed = Box::pin(seed_unacknowledged_local_notification(
            &base.join("router"),
            &mut central,
        ))
        .await?;
        Box::pin(Self::resume(base, central, seed)).await
    }
    pub(super) async fn open(base: &Path) -> Result<Self, TestError> {
        let mut central = TestFleet::start_mail_as(&base.join("central"), None).await?;
        let seed = Box::pin(seed_unacknowledged_notification(
            &base.join("router"),
            &mut central,
        ))
        .await?;
        Box::pin(Self::resume(base, central, seed)).await
    }
    pub(super) async fn resume(
        base: &Path,
        central: TestFleet,
        seed: NotificationSeed,
    ) -> Result<Self, TestError> {
        let mut harness =
            MailHarness::open_configured(base, None, false, true, None, Some(central)).await?;
        let fleet = Arc::new(
            FleetClient::open(
                &harness.root,
                harness.projects.clone(),
                Some(harness.requests.clone()),
            )
            .await?,
        );
        let machine = MachineIdentity::new(
            "00000000-0000-4000-8000-000000000001"
                .to_owned()
                .try_into()?,
            Some("notice fixture"),
        )?;
        let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
            registry: harness.registry.clone(),
            fleet: fleet.clone(),
            identity: machine,
        })?);
        let identity = ApplicationFixture::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_project_admission(admission.clone())
        .with_project_replication(harness.projects.clone())
        .with_automation_store(harness.mail.automation.clone())
        .with_subscription_delivery_service(
            harness.mail.actors.clone(),
            Arc::new(SessionDeliveryRouter::new(Vec::new())),
        )
        .with_interaction_store(harness.mail.owner.clone())
        .with_installation_peer(harness.peer())
        .map_err(io::Error::other)?
        .with_mail_delivery_runtime(harness.mail.mail.clone())
        .map_err(io::Error::other)?;
        let publisher = Arc::new(NotificationPublisher::new(
            admission.clone(),
            harness.mail.owner.clone(),
            Arc::downgrade(&harness.mail.mail),
        ));
        harness.admission = Arc::downgrade(&admission);
        harness.fleet = Arc::downgrade(&fleet);
        harness.publisher = Arc::downgrade(&publisher);
        let application = (identity.with_notification_publisher(publisher.clone())).build();
        let store = harness
            .registry
            .existing_project(&seed.project)
            .await?
            .ok_or("seeded file")?;
        let mut read = store.snapshot().await?;
        let root = read
            .records(RecordKind::Thread)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::Thread(thread) => Some(thread.thread_id),
                _ => None,
            })
            .ok_or("seeded root")?;
        read.finish().await?;
        Ok(Self {
            harness,
            admission,
            fleet,
            publisher,
            application,
            project: seed.project,
            seed_item: seed.item,
            seed_epoch: seed.epoch,
            root,
        })
    }
    pub(super) async fn start(&mut self) -> Result<(), TestError> {
        self.harness.start_mail();
        self.harness.release_welcome()?;
        let mut initial = Vec::new();
        loop {
            let event = self.harness.next_manual_event().await?;
            let welcome = matches!(event, ClientEvent::Welcome { .. });
            initial.push(event);
            if welcome {
                break;
            }
        }
        self.harness.resume_events(initial).await
    }
    pub(super) async fn post(
        &self,
        author: &str,
        recipient: Option<ServiceId>,
    ) -> Result<PostId, TestError> {
        let post = PostId::generate();
        let addressees = recipient
            .map(|service| {
                NotifyTarget::Actor(Identity::Session {
                    session: SessionRef {
                        endpoint: SessionEndpointRef {
                            service_id: service,
                            endpoint_id: "codex-local"
                                .to_owned()
                                .try_into()
                                .expect("static endpoint"),
                        },
                        session_id: "directory-recipient"
                            .to_owned()
                            .try_into()
                            .expect("static session"),
                    },
                })
            })
            .into_iter()
            .collect();
        let result = self
            .admission
            .submit(&ProjectRequest {
                request_id: RequestId::generate(),
                project_id: self.project.clone(),
                actor: Identity::Human {
                    human_id: author.to_owned().try_into()?,
                },
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                operation: BoardOperation::ThreadPost {
                    post_id: post.clone(),
                    thread_id: self.root.clone(),
                    text: "live directory proof".to_owned().try_into()?,
                    references: Vec::new(),
                    addressees,
                },
            })
            .await?;
        assert!(
            matches!(
                result.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{result:?}"
        );
        Ok(post)
    }
    pub(super) async fn intent_for(&self, post: &PostId) -> Result<NotifyIntent, TestError> {
        let store = self
            .harness
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("project file")?;
        let mut read = store.snapshot().await?;
        let intent = read
            .records(RecordKind::NotifyIntent)
            .await?
            .into_iter()
            .find_map(|record| match record {
                Record::NotifyIntent(intent) if intent.cause == post.clone().into() => Some(intent),
                _ => None,
            })
            .ok_or("post intent")?;
        read.finish().await?;
        Ok(intent)
    }
    pub(super) async fn wait_intent(
        &self,
        post: &PostId,
        ready: impl Fn(&IntentState) -> bool,
    ) -> Result<NotifyIntent, TestError> {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let intent = self.intent_for(post).await?;
                if ready(intent.state()) {
                    return Ok::<_, TestError>(intent);
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .map_err(|_| {
            io::Error::other("directory result did not decide the actual project intent")
        })?
    }
    pub(super) async fn frozen_peer(
        &self,
        intent: &NotifyIntent,
    ) -> Result<fleet_sync_protocol::PeerId, TestError> {
        let IntentState::HandedOff { item } = intent.state() else {
            return Err("expected frozen handoff".into());
        };
        let store = self
            .harness
            .registry
            .existing_project(&self.project)
            .await?
            .ok_or("project file")?;
        let mut read = store.snapshot().await?;
        let handoff = read.mail_handoff(item).await?.ok_or("frozen row")?;
        read.finish().await?;
        let message_board_storage::MailHandoffItem::Delivery(envelope) = handoff.item else {
            return Err("not delivery".into());
        };
        let fleet_sync_protocol::DeliveryEnvelope::Item(envelope) = *envelope else {
            return Err("not item".into());
        };
        assert_eq!(item.as_str(), envelope.id().as_uuid().to_string());
        Ok(envelope.recipient().peer)
    }
    pub(super) async fn show_local_push(
        &self,
        push: &collaboration_protocol::PushId,
    ) -> Result<collaboration_protocol::PushRecordShowResult, TestError> {
        let record = self
            .harness
            .mail
            .owner
            .get_push_record(push)
            .await?
            .ok_or("native notification push")?;
        Ok(self
            .application
            .messages()
            .push_show(collaboration_protocol::PushRecordShowParams {
                caller: record.target,
                reference: push.as_str().to_owned(),
            })
            .await?)
    }

    async fn drain_publisher(self) -> (MailHarness, Arc<ProjectAdmission>) {
        self.publisher.shutdown().await;
        self.harness.mail.actors.shutdown().await;
        drop(self.application);
        drop(self.publisher);
        drop(self.fleet);
        (self.harness, self.admission)
    }
    pub(super) async fn close(self) -> Result<(), TestError> {
        let (harness, admission) = self.drain_publisher().await;
        harness.close().await?;
        Arc::try_unwrap(admission)
            .map_err(|_| io::Error::other("notification admission retained"))?
            .close()
            .await?;
        Ok(())
    }
    pub(super) async fn close_keeping_central(self) -> Result<TestFleet, TestError> {
        let (harness, admission) = self.drain_publisher().await;
        let central = harness.close_keeping_central().await?;
        Arc::try_unwrap(admission)
            .map_err(|_| io::Error::other("notification admission retained"))?
            .close()
            .await?;
        Ok(central)
    }
}
