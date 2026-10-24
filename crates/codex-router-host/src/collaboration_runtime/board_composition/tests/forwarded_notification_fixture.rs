//! Existing native publisher and custody owners attached to a real enrolled holder.
use super::snapshot_restart::board_fixture::native_mail_owner::NativeMailOwner;
use super::*;
use collaboration_service::{
    BoardAvailability, CollaborationApplication, MailPushDeliveryProps, NotificationPublisher,
    ServiceIdentity, SessionDeliveryRouter, SubscriptionDeliveryService,
    SubscriptionDeliveryServiceProps, SystemSubscriptionClock,
};
use durable_steps::{AtomicStore, CallFailure, StoreError};
use fleet_sync_protocol::ItemId;
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use tokio::sync::Mutex;

struct NoObservationCalls;
impl CallRouter for NoObservationCalls {
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}

pub(super) struct HolderNotifications {
    pub application: CollaborationApplication,
    pub publisher: Arc<NotificationPublisher>,
    pub owner: Arc<InteractionStore>,
    actors: SubscriptionDeliveryService,
    mail: NativeMailOwner,
}

impl HolderNotifications {
    pub async fn hold_progress_writer(
        &self,
    ) -> Result<
        (
            tokio::sync::oneshot::Sender<()>,
            tokio::task::JoinHandle<Result<(), StoreError>>,
        ),
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let observer = InteractionAtomicStore::new(
            self.owner.clone(),
            Arc::new(TokioHost::new(Arc::new(NoObservationCalls), SystemClock)),
        );
        let (entered, waiting) = tokio::sync::oneshot::channel();
        let (release, released) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            observer
                .transaction(move |_scope| {
                    Box::pin(async move {
                        entered
                            .send(())
                            .map_err(|_| StoreError::backend("progress observer closed"))?;
                        released.await.map_err(StoreError::backend)?;
                        Ok(())
                    })
                })
                .await
        });
        timeout(BOUND, waiting).await??;
        Ok((release, task))
    }

    pub async fn prepare(board: &mut BoardComposition, root: &Path) -> TestResultOwner {
        let machine = identity()?;
        let session = board.mail_session().ok_or("holder mail session")?;
        let peer = session.peer;
        let owner =
            Arc::new(InteractionStore::open(root.join("notification-interactions.sqlite")).await?);
        let carrier = Arc::new(SessionDeliveryRouter::new(Vec::new()));
        let actors = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Available(Arc::new(Mutex::new(
                board.admission.subscription_window_store(owner.clone()),
            ))),
            push_store: owner.clone(),
            delivery: carrier.clone(),
            presence: carrier.clone(),
            machine_identity: machine.clone(),
            clock: Arc::new(SystemSubscriptionClock),
        });
        actors.start().await?;
        let identity = ServiceIdentity::new(
            origin()?.installation.as_str(),
            "00000000-0000-4000-8000-000000000002",
        )?
        .with_machine_identity(machine.clone())
        .map_err(io::Error::other)?
        .with_project_admission(board.admission.clone())
        .with_project_replication(board.replication.clone().ok_or("holder replication")?)
        .with_subscription_delivery_service(actors.clone(), carrier.clone());
        let (identity, mail) = NativeMailOwner::start_with_session(
            identity,
            peer,
            MailPushDeliveryProps {
                interactions: owner.clone(),
                actors: actors.clone(),
                presence: carrier,
                machine,
                clock: Arc::new(SystemSubscriptionClock),
            },
            Some(session),
        )?;
        let runtime = mail.runtime_handle();
        board.attach_mail(&runtime);
        let publisher = Arc::new(NotificationPublisher::new(
            board.admission.clone(),
            owner.clone(),
            Arc::downgrade(&runtime),
        ));
        board.attach_publisher(&publisher);
        let application =
            CollaborationApplication::new(identity.with_notification_publisher(publisher.clone()));
        Ok(Self {
            application,
            publisher,
            owner,
            actors,
            mail,
        })
    }

    // Read only through the runtime's physical owner; this observer executes no task.
    pub async fn rows(
        &self,
    ) -> Result<
        Vec<fleet_sync_delivery::test_support::DeliveryObservation>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let observer = InteractionAtomicStore::new(
            self.owner.clone(),
            Arc::new(TokioHost::new(Arc::new(NoObservationCalls), SystemClock)),
        );
        Ok(observer
            .transaction(|scope| {
                Box::pin(async move {
                    fleet_sync_delivery::test_support::snapshot(scope)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?)
    }

    pub async fn local_push(
        &self,
        item: ItemId,
    ) -> Result<
        Option<collaboration_protocol::PushRecordShowResult>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let push: collaboration_protocol::PushId = item.as_uuid().to_string().try_into()?;
        let Some(record) = self.owner.get_push_record(&push).await? else {
            return Ok(None);
        };
        Ok(Some(
            self.application
                .messages()
                .push_show(collaboration_protocol::PushRecordShowParams {
                    caller: record.target,
                    reference: push.as_str().to_owned(),
                })
                .await?,
        ))
    }

    pub async fn close(self) -> TestResult {
        self.publisher.shutdown().await;
        self.actors.shutdown().await;
        drop(self.application);
        drop(self.publisher);
        drop(self.actors);
        drop(self.owner);
        self.mail.close().await?;
        Ok(())
    }
}

type TestResultOwner = Result<HolderNotifications, Box<dyn std::error::Error + Send + Sync>>;
