#[path = "follow_admission_port.rs"]
mod follow_admission_port;
#[cfg(test)]
#[path = "idle_owner_close_probe.rs"]
pub(super) mod idle_owner_close_probe;
#[cfg(test)]
#[path = "tests/idle_owner_close_proofs.rs"]
mod idle_owner_close_proofs;
#[cfg(test)]
#[path = "intent_progress_tests.rs"]
mod intent_progress_tests;
#[path = "native_mail_readiness.rs"]
mod native_mail_readiness;
#[path = "reader_owner_registry.rs"]
mod reader_owner_registry;
use reader_owner_registry::ReaderOwnerHandle;
#[cfg(test)]
#[path = "poll_handover_probe.rs"]
mod poll_handover_probe;
#[path = "poll_mail_attempt.rs"]
mod poll_mail_attempt;
pub(super) use follow_admission_port::FollowAdmissionSlot;
#[cfg(test)]
pub(super) use poll_handover_probe::{PollHandoverProbeSlot, PollHandoverStage};
#[cfg(test)]
#[path = "tests/startup_recovery_regressions.rs"]
mod startup_recovery_regressions;
#[path = "subscription_startup_recovery.rs"]
mod subscription_startup_recovery;
use super::ReaderSubscriptionSnapshot;
use super::reader_scan_scope::ReaderScanScope;
use super::{
    BoardAvailability, SubscriptionClock,
    reader_delivery_owner::{
        ReaderDeliveryCommand, ReaderDeliveryOwner, ReaderDeliveryOwnerProps,
        ReaderReconcilePurpose,
    },
    subscription_push::{SubscriptionPushStore, SubscriptionPushStoreProps},
    subscription_wait::SubscriptionWaitFilter,
};
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{PushId, PushRecord, SessionRef};
use message_board::{BoardError, Identity, ProjectId};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing::instrument::WithSubscriber;

pub struct SubscriptionDeliveryServiceProps {
    pub board_availability: BoardAvailability,
    pub push_store: Arc<interaction_storage::InteractionStore>,
    pub delivery: Arc<dyn SessionMessageDelivery>,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub machine_identity: MachineIdentity,
    pub clock: Arc<dyn SubscriptionClock>,
}

#[cfg(test)]
#[derive(Clone, Debug)]
pub(super) enum OwnerObservation {
    ReaderPassFailed(BoardError),
    Started,
    RowsLoaded(usize),
    SelectingDueRoots,
    DueRoots(usize),
    Selected,
    Settled,
    SettlementRetry,
    PushSettlementRetry,
    WaitQueued,
    ReconcileRequested,
    ActivityLagged,
    Sleeping(Option<tokio::time::Instant>),
    DirectMessageQueued,
    MailAttemptQueued(PushId),
    PollCancellationDeferred(fleet_sync_protocol::ItemId),
    PollPublisherUnavailable(fleet_sync_protocol::ItemId),
    NativeHeldChecked { item: PushId, running: bool },
    DirectMessageHeld,
    DirectMessageSettled,
    HeldSubscriptionPushesSettled(u64),
    Stopped,
}

pub(super) type NativeMailReadiness =
    Arc<std::sync::OnceLock<std::sync::Weak<crate::MailDeliveryRuntime>>>;

struct ServiceInner {
    notification_startup_attempts: std::sync::OnceLock<Arc<std::collections::HashSet<PushId>>>,
    #[cfg(test)]
    poll_handover_probe: PollHandoverProbeSlot,
    #[cfg(test)]
    idle_owner_close_probe: idle_owner_close_probe::IdleOwnerCloseProbeSlot,
    native_mail: NativeMailReadiness,
    follow_admission: FollowAdmissionSlot,
    board_availability: BoardAvailability,
    push: Arc<SubscriptionPushStore>,
    presence: Arc<dyn TargetPresenceProbe>,
    clock: Arc<dyn SubscriptionClock>,
    owners: Mutex<HashMap<Identity, ReaderOwnerHandle>>,
    start_gate: Mutex<bool>,
    shutdown: CancellationToken,
    tasks: TaskTracker,
    #[cfg(test)]
    observations: tokio::sync::broadcast::Sender<OwnerObservation>,
    /// Kept apart from owner observations, whose order existing proofs assert.
    #[cfg(test)]
    reconcile_queued: tokio::sync::Notify,
    #[cfg(test)]
    startup_passes: tokio::sync::broadcast::Sender<Vec<ProjectId>>,
}

#[derive(Clone)]
enum ServiceHandleLifetime {
    Owner(Arc<()>),
    Background,
}

#[derive(Clone)]
pub struct SubscriptionDeliveryService {
    inner: Arc<ServiceInner>,
    lifetime: ServiceHandleLifetime,
}

impl SubscriptionDeliveryService {
    #[must_use]
    pub fn subscription_clock(&self) -> Arc<dyn SubscriptionClock> {
        Arc::clone(&self.inner.clock)
    }

    pub fn new(props: SubscriptionDeliveryServiceProps) -> Self {
        #[cfg(test)]
        let (observations, _) = tokio::sync::broadcast::channel(1024);
        let shutdown = CancellationToken::new();
        let push = Arc::new(SubscriptionPushStore::new(SubscriptionPushStoreProps {
            store: props.push_store,
            delivery: props.delivery,
            machine: props.machine_identity,
            clock: props.clock.clone(),
            shutdown: shutdown.clone(),
            #[cfg(test)]
            observations: observations.clone(),
        }));
        Self {
            lifetime: ServiceHandleLifetime::Owner(Arc::new(())),
            inner: Arc::new(ServiceInner {
                notification_startup_attempts: std::sync::OnceLock::new(),
                #[cfg(test)]
                poll_handover_probe: Arc::new(std::sync::Mutex::new(None)),
                #[cfg(test)]
                idle_owner_close_probe: Arc::new(std::sync::Mutex::new(None)),
                native_mail: Arc::new(std::sync::OnceLock::new()),
                follow_admission: Arc::new(std::sync::RwLock::new(None)),
                board_availability: props.board_availability,
                push,
                presence: props.presence,
                clock: props.clock,
                owners: Mutex::new(HashMap::new()),
                start_gate: Mutex::new(false),
                shutdown,
                tasks: TaskTracker::new(),
                #[cfg(test)]
                observations,
                #[cfg(test)]
                reconcile_queued: tokio::sync::Notify::new(),
                #[cfg(test)]
                startup_passes: tokio::sync::broadcast::channel(32).0,
            }),
        }
    }

    /// Host composition calls this before creating the mail runtime. The immutable
    /// snapshot distinguishes previous-process attempts from fresh SDK callbacks.
    pub async fn capture_notification_startup_snapshot(&self) -> Result<(), BoardError> {
        if self.inner.notification_startup_attempts.get().is_none() {
            let attempts = self
                .inner
                .push
                .store
                .notification_startup_attempt_ids()
                .await
                .map_err(|error| {
                    tracing::warn!(%error, "notification startup snapshot failed");
                    BoardError::board_unavailable()
                })?;
            let _ = self
                .inner
                .notification_startup_attempts
                .set(Arc::new(attempts.into_iter().collect()));
        }
        Ok(())
    }

    pub async fn start(&self) -> Result<(), BoardError> {
        let mut started = self.inner.start_gate.lock().await;
        if self.inner.shutdown.is_cancelled() {
            return Err(BoardError::board_unavailable());
        }
        if *started {
            return Ok(());
        }
        self.capture_notification_startup_snapshot().await?;
        let (restored, probe_store) = match &self.inner.board_availability {
            BoardAvailability::Available(store) => {
                let mut store = store.lock().await;
                let restored = store
                    .restore_active_and_draining(self.inner.clock.now())
                    .await?;
                // Windows contain only shared storage handles. Deferred native reads must
                // not hold the service's window-store gate across another pool timeout.
                (restored, Some(store.clone()))
            }
            BoardAvailability::Unavailable => (
                super::subscription_windows::SubscriptionStartupRestoration::default(),
                None,
            ),
        };
        let notification_targets = match self.inner.push.store.notification_recovery_targets().await
        {
            Ok(targets) => targets,
            Err(error) if recovery_connection_failed(&error) => {
                tracing::warn!(%error, "notification recovery connection unavailable");
                return Err(BoardError::board_unavailable());
            }
            Err(error) => {
                tracing::warn!(%error, "notification recipient discovery failed; healthy readers continue and recover in their first pass");
                Vec::new()
            }
        };
        let dm_targets = match self.inner.push.restore_direct_messages().await {
            Ok(targets) => targets,
            Err(error) if recovery_connection_failed(&error) => {
                tracing::warn!(%error, "direct-message recovery connection unavailable");
                return Err(BoardError::board_unavailable());
            }
            Err(error) => {
                tracing::warn!(%error, "direct-message recovery failed; healthy subscription readers continue");
                Vec::new()
            }
        };
        *started = true;
        self.ensure_restored_owners(restored.records, restored.expiry_readers)
            .await?;
        for target in dm_targets.into_iter().chain(notification_targets) {
            self.ensure_owner(reader_for_target(&target)?).await?;
        }
        if let Some(store) = probe_store {
            self.defer_startup_discovery(store, restored.retry_projects);
        }
        Ok(())
    }

    pub async fn reconcile_reader(&self, reader: Identity) -> Result<(), BoardError> {
        self.reconcile_scope(reader, ReaderScanScope::All)
            .await
            .map(|_| ())
    }
    pub(crate) async fn reconcile_project(
        &self,
        reader: Identity,
        project: ProjectId,
    ) -> Result<(), BoardError> {
        self.reconcile_scope(reader, ReaderScanScope::project(project))
            .await
            .map(|_| ())
    }
    pub(crate) async fn reader_subscriptions(
        &self,
        reader: Identity,
    ) -> Result<ReaderSubscriptionSnapshot, BoardError> {
        let mut snapshot = self.reconcile_scope(reader, ReaderScanScope::All).await?;
        let windows = self
            .inner
            .board_availability
            .require_store()?
            .lock()
            .await
            .clone();
        // Native observations decorate this response after reconciliation. They
        // cannot change allocation facts or turn a view failure into a planning failure.
        windows
            .observe_subscription_deliveries(&mut snapshot)
            .await?;
        Ok(snapshot)
    }
    async fn reconcile_scope(
        &self,
        reader: Identity,
        scope: ReaderScanScope,
    ) -> Result<ReaderSubscriptionSnapshot, BoardError> {
        self.inner.board_availability.require_store()?;
        self.require_started().await?;
        self.reconcile_owned_scope(reader, scope, ReaderReconcilePurpose::Refresh)
            .await
    }
    async fn reconcile_owned_scope(
        &self,
        reader: Identity,
        scope: ReaderScanScope,
        purpose: ReaderReconcilePurpose,
    ) -> Result<ReaderSubscriptionSnapshot, BoardError> {
        loop {
            let (reply, response) = oneshot::channel();
            self.submit_reader_command(
                reader.clone(),
                scope.clone(),
                ReaderDeliveryCommand::Reconcile {
                    scope: scope.clone(),
                    purpose,
                    reply,
                },
            )
            .await?;
            #[cfg(test)]
            self.inner.reconcile_queued.notify_one();
            match response.await {
                Ok(outcome) => return outcome,
                // The owner closed before its pass answered; a replacement owner rescans.
                Err(_) if self.inner.shutdown.is_cancelled() => {
                    return Err(BoardError::board_unavailable());
                }
                Err(_) => {}
            }
        }
    }

    /// The record already exists: route its known-unsent work through the target's owner.
    pub async fn deliver_direct_message(
        &self,
        target: SessionRef,
        push_id: PushId,
    ) -> Result<PushRecord, BoardError> {
        self.require_started().await?;
        let reader = reader_for_target(&target)?;
        loop {
            let (reply, response) = oneshot::channel();
            let sender = self.ensure_owner(reader.clone()).await?;
            if sender
                .send(ReaderDeliveryCommand::DmQueued {
                    push_id: push_id.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                continue;
            }
            #[cfg(test)]
            let _ = self
                .inner
                .observations
                .send(OwnerObservation::DirectMessageQueued);
            match response.await {
                Ok(result) => return result,
                Err(_) => {
                    self.require_started().await?;
                }
            }
        }
    }

    /// The caller owns admission and outcome persistence. A lost response after
    /// acceptance is Unknown; this method never requeues an accepted attempt.
    pub async fn attempt_mail_once(
        &self,
        kind: collaboration_protocol::PushKind,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
    ) -> collaboration_contracts::NativeDeliveryResult {
        self.attempt_checked_mail(
            kind,
            request,
            super::reader_delivery_owner::MailAttemptCheck::CarrierOnly,
        )
        .await
    }

    pub(crate) async fn attempt_follow_mail_once(
        &self,
        project: ProjectId,
        reference: fleet_sync_protocol::DeliveryItemRef,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
    ) -> collaboration_contracts::NativeDeliveryResult {
        self.attempt_checked_mail(
            collaboration_protocol::PushKind::SubscriptionActivity,
            request,
            super::reader_delivery_owner::MailAttemptCheck::FrozenFollow { project, reference },
        )
        .await
    }

    async fn attempt_checked_mail(
        &self,
        kind: collaboration_protocol::PushKind,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        check: super::reader_delivery_owner::MailAttemptCheck,
    ) -> collaboration_contracts::NativeDeliveryResult {
        use collaboration_contracts::NativeDeliveryResult;
        use collaboration_contracts::NativeRefusalCode;
        use collaboration_contracts::UnsentCause;
        use collaboration_protocol::PushKind;
        let not_started = |retryable, reason: &str, cause| {
            NativeDeliveryResult::not_submitted(reason, retryable, cause)
        };
        let unavailable = UnsentCause::Holdable {
            otherwise: NativeRefusalCode::CarrierUnavailable,
        };
        if !matches!(
            kind,
            PushKind::DirectMessage | PushKind::BoardNotification | PushKind::SubscriptionActivity
        ) {
            return not_started(false, "invalid native mail kind", UnsentCause::InvalidInput);
        }
        if &request.target.endpoint.service_id != self.inner.push.machine.service_id() {
            return not_started(
                false,
                "mail target belongs to another Router",
                UnsentCause::InvalidInput,
            );
        }
        if self.require_started().await.is_err() {
            return not_started(
                true,
                "mail carrier is unavailable before the attempt",
                unavailable,
            );
        }
        let Ok(reader) = reader_for_target(&request.target) else {
            return not_started(
                false,
                "invalid native mail target",
                UnsentCause::InvalidInput,
            );
        };
        let Ok(sender) = self.ensure_owner(reader).await else {
            return not_started(
                true,
                "mail carrier closed before accepting the attempt",
                unavailable,
            );
        };
        #[cfg(test)]
        let push_id = request.payload.push_id.clone();
        let (reply, response) = oneshot::channel();
        let command = ReaderDeliveryCommand::MailAttempt {
            kind,
            request: Box::new(request),
            check,
            reply,
        };
        let sent = tokio::select! {
            ()=self.inner.shutdown.cancelled()=>return not_started(true,"mail carrier closed before accepting the attempt", unavailable),
            sent=sender.send(command)=>sent,
        };
        if sent.is_err() {
            return not_started(
                true,
                "mail carrier closed before accepting the attempt",
                unavailable,
            );
        }
        #[cfg(test)]
        let _ = self
            .inner
            .observations
            .send(OwnerObservation::MailAttemptQueued(push_id));
        tokio::select! {
            ()=self.inner.shutdown.cancelled()=>NativeDeliveryResult::unknown(),
            receipt=response=>receipt.unwrap_or_else(|_|NativeDeliveryResult::unknown()),
        }
    }

    pub(crate) async fn notification_listing_result(
        &self,
        target: SessionRef,
        listing: PushId,
    ) -> collaboration_contracts::NativeDeliveryResult {
        use collaboration_contracts::NativeDeliveryResult;
        loop {
            let record = match self.inner.push.store.get_push_record(&listing).await {
                Ok(Some(record))
                    if record.target == target
                        && record.kind == collaboration_protocol::PushKind::NotificationListing =>
                {
                    record
                }
                _ => return NativeDeliveryResult::unknown(),
            };
            if matches!(
                record.delivery_state,
                collaboration_protocol::PushDeliveryState::Delivered
                    | collaboration_protocol::PushDeliveryState::OutcomeUnknown
                    | collaboration_protocol::PushDeliveryState::Rejected
            ) {
                return crate::mail_push_outcome::listing_native_result(&record);
            }
            if self.require_started().await.is_err() {
                return NativeDeliveryResult::unknown();
            }
            let Ok(reader) = reader_for_target(&target) else {
                return NativeDeliveryResult::unknown();
            };
            let Ok(sender) = self.ensure_owner(reader).await else {
                return NativeDeliveryResult::unknown();
            };
            let (reply, response) = oneshot::channel();
            if sender
                .send(ReaderDeliveryCommand::ListingResult {
                    listing: listing.clone(),
                    reply,
                })
                .await
                .is_err()
            {
                continue;
            }
            return tokio::select! {() = self.inner.shutdown.cancelled() =>NativeDeliveryResult::unknown(),result=response=>result.unwrap_or_else(|_|NativeDeliveryResult::unknown())};
        }
    }

    /// Internal batch handoff. The public stored-notice conversion belongs to the surface.
    pub async fn wait(
        &self,
        reader: Identity,
        filter: SubscriptionWaitFilter,
        max_wait_seconds: u64,
        maximum_root_notice_bytes: usize,
    ) -> Result<Option<super::SubscriptionWaitResult>, BoardError> {
        self.inner.board_availability.require_store()?;
        self.require_started().await?;
        if max_wait_seconds > 1500 {
            return Err(BoardError::invalid_field(
                "maxWaitSeconds",
                "must be at most 1500 seconds",
            ));
        }
        filter.validate()?;
        let filter = filter
            .resolve(self.inner.board_availability.require_store()?)
            .await?;
        let deadline = self
            .inner
            .clock
            .monotonic_now()
            .checked_add(std::time::Duration::from_secs(max_wait_seconds))
            .ok_or_else(BoardError::board_unavailable)?;
        loop {
            let (reply, response) = oneshot::channel();
            let sender = self.ensure_owner(reader.clone()).await?;
            if sender
                .send(ReaderDeliveryCommand::Wait {
                    filter: filter.clone(),
                    deadline,
                    maximum_root_notice_bytes,
                    reply,
                })
                .await
                .is_err()
            {
                self.require_started().await?;
                continue;
            }
            return response
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        }
    }

    pub fn cancel(&self) {
        self.inner.shutdown.cancel();
    }
    pub async fn shutdown(&self) {
        self.cancel();
        self.inner.tasks.close();
        self.inner.tasks.wait().await;
        self.inner.owners.lock().await.clear();
    }

    async fn require_started(&self) -> Result<(), BoardError> {
        if !*self.inner.start_gate.lock().await || self.inner.shutdown.is_cancelled() {
            Err(BoardError::board_unavailable())
        } else {
            Ok(())
        }
    }

    #[cfg(debug_assertions)]
    pub async fn debug_advance_notification_clock_millis(
        &self,
        now_ms: i64,
    ) -> Result<usize, BoardError> {
        let now = chrono::DateTime::from_timestamp_millis(now_ms).ok_or_else(|| {
            BoardError::invalid_field("nowMs", "must be UTC epoch milliseconds in range")
        })?;
        self.debug_advance_notification_clock(now).await
    }

    #[cfg(debug_assertions)]
    pub async fn debug_advance_notification_clock(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<usize, BoardError> {
        self.require_started().await?;
        self.inner
            .clock
            .debug_advance_to(now)
            .map_err(|message| BoardError::invalid_field("nowMs", message))?;
        let readers: Vec<_> = self.inner.owners.lock().await.keys().cloned().collect();
        let mut completed = 0;
        for reader in readers {
            loop {
                let (reply, response) = oneshot::channel();
                self.submit_reader_command(
                    reader.clone(),
                    ReaderScanScope::All,
                    ReaderDeliveryCommand::DebugScan(reply),
                )
                .await?;
                let result = tokio::select! {
                    () = self.inner.shutdown.cancelled() => return Err(BoardError::board_unavailable()),
                    result = response => result,
                };
                match result {
                    Ok(outcome) => {
                        outcome?;
                        break;
                    }
                    // An idle owner may close after accepting the command. Its
                    // channel closure prompts one scan on its replacement owner.
                    Err(_) => continue,
                }
            }
            completed += 1;
        }
        Ok(completed)
    }

    #[cfg(test)]
    pub(super) fn observe_owners(&self) -> tokio::sync::broadcast::Receiver<OwnerObservation> {
        self.inner.observations.subscribe()
    }

    #[cfg(test)]
    pub(super) async fn synchronize_reader(&self, reader: Identity) {
        let (reply, response) = oneshot::channel();
        self.ensure_owner(reader)
            .await
            .expect("active owner")
            .send(ReaderDeliveryCommand::Barrier(reply))
            .await
            .expect("owner command");
        response
            .await
            .expect("owner reached storage-derived sleep boundary");
    }

    #[cfg(test)]
    pub(super) async fn request_reconcile_receipt(
        &self,
        reader: Identity,
    ) -> oneshot::Receiver<Result<ReaderSubscriptionSnapshot, BoardError>> {
        let (reply, response) = oneshot::channel();
        self.ensure_owner(reader)
            .await
            .expect("active owner")
            .send(ReaderDeliveryCommand::Reconcile {
                scope: ReaderScanScope::All,
                purpose: ReaderReconcilePurpose::Refresh,
                reply,
            })
            .await
            .expect("owner command");
        response
    }
    /// Resolves once a `reconcile_reader` call has queued its command with an owner.
    #[cfg(test)]
    pub(super) async fn reconcile_queued(&self) {
        self.inner.reconcile_queued.notified().await;
    }
    #[cfg(test)]
    pub(super) async fn request_reconcile(&self, reader: Identity) {
        drop(self.request_reconcile_receipt(reader).await);
    }
}

impl Drop for SubscriptionDeliveryService {
    fn drop(&mut self) {
        if matches!(&self.lifetime,ServiceHandleLifetime::Owner(lifetime) if Arc::strong_count(lifetime) == 1)
        {
            self.cancel();
        }
    }
}

fn reader_for_target(target: &SessionRef) -> Result<Identity, BoardError> {
    let session = serde_json::from_value(
        serde_json::to_value(target).map_err(|_| BoardError::board_unavailable())?,
    )
    .map_err(|_| BoardError::board_unavailable())?;
    Ok(Identity::Session { session })
}

fn recovery_connection_failed(error: &interaction_storage::StorageError) -> bool {
    if let interaction_storage::StorageError::Database(sqlx::Error::Database(database)) = error {
        // SQLite extended error codes retain the primary connection/storage class.
        return database
            .code()
            .and_then(|code| code.parse::<i32>().ok())
            .is_some_and(|code| matches!(code & 0xff, 7 | 10 | 11 | 14 | 26));
    }
    matches!(
        error,
        interaction_storage::StorageError::Database(
            sqlx::Error::Io(_)
                | sqlx::Error::Tls(_)
                | sqlx::Error::Protocol(_)
                | sqlx::Error::WorkerCrashed
                | sqlx::Error::PoolClosed
                | sqlx::Error::PoolTimedOut
        )
    )
}

#[cfg(test)]
#[path = "tests/reconcile_retry_proofs.rs"]
mod reconcile_retry_proofs;
