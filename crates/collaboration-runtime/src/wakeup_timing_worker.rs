//! Host-owned timer task turns due wake-ups into durable obligations, independently of native I/O.
use automation_storage::{AutomationStore, StorageError};
use collaboration_protocol::SavedMessage;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct WakeTimingWorker {
    store: Arc<Mutex<AutomationStore>>,
    native: crate::wakeup_delivery_sender::WakeDeliverySender,
    #[cfg(any(test, feature = "test-support"))]
    accepted_observer: Option<tokio::sync::mpsc::UnboundedSender<agent_automation::DeliveryId>>,
}
impl WakeTimingWorker {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        delivery: Arc<dyn collaboration_contracts::SessionMessageDelivery>,
        configuration: crate::AutomationConfigurationHandle,
        machine_identity: collaboration_contracts::MachineIdentity,
        interactions: Arc<interaction_storage::InteractionStore>,
        clock: Arc<dyn runtime_primitives::Clock>,
    ) -> Self {
        let native = crate::wakeup_delivery_sender::WakeDeliverySender {
            delivery,
            configuration,
            machine_identity,
            interactions,
            clock,
        };
        Self {
            store,
            native,
            #[cfg(any(test, feature = "test-support"))]
            accepted_observer: None,
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    pub fn observe_accepted_deliveries(
        &mut self,
    ) -> tokio::sync::mpsc::UnboundedReceiver<agent_automation::DeliveryId> {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.accepted_observer = Some(sender);
        receiver
    }
    /// Start only after the Host owns its listeners and previous workers have retired.
    pub async fn run(self, shutdown: CancellationToken) {
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut dispatches = tokio::task::JoinSet::<Result<(), StorageError>>::new();
        let mut recovered = false;
        let mut reported_failure = false;
        loop {
            tokio::select! {
                biased;
                _=shutdown.cancelled()=>{
                    dispatches.abort_all();
                    while dispatches.join_next().await.is_some() {}
                    return;
                },
                completed=dispatches.join_next(), if !dispatches.is_empty()=>{
                    if !matches!(completed,Some(Ok(Ok(())))) {tracing::warn!("automation delivery task stopped; persisted attempt must be inspected before replay");}
                    continue;
                }
                _=interval.tick()=>{}
            }
            let now = self.native.clock.now().unix_millis();
            let result = if recovered {
                if self.native.configuration.current().await.is_none() {
                    continue;
                }
                match self.evaluate_due(now).await {
                    Ok(()) => self.dispatch_pending(&mut dispatches, now).await,
                    Err(error) => Err(error),
                }
            } else {
                match self
                    .store
                    .lock()
                    .await
                    .recover_interrupted_deliveries(now)
                    .await
                {
                    Ok(_) => {
                        recovered = true;
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            };
            match result {
                Ok(()) => reported_failure = false,
                Err(_) => {
                    if !reported_failure {
                        tracing::warn!(
                            "automation timing storage unavailable; no new delivery is dispatched by this pass"
                        );
                    }
                    reported_failure = true;
                }
            }
        }
    }
    async fn dispatch_pending(
        &self,
        tasks: &mut tokio::task::JoinSet<Result<(), StorageError>>,
        now_ms: i64,
    ) -> Result<(), StorageError> {
        let slots = 16_usize.saturating_sub(tasks.len());
        if slots == 0 {
            return Ok(());
        }
        let ids = self
            .store
            .lock()
            .await
            .eligible_delivery_ids(
                now_ms,
                u32::try_from(slots).map_err(|_| StorageError::InvalidRecord)?,
            )
            .await?;
        for id in ids {
            let store = Arc::clone(&self.store);
            let native = self.native.clone();
            #[cfg(any(test, feature = "test-support"))]
            let observer = self.accepted_observer.clone();
            tasks.spawn(async move {
                #[cfg(any(test, feature = "test-support"))]
                let observed = (Arc::clone(&store), id.clone());
                let result = native.dispatch(store, id).await;
                #[cfg(any(test, feature = "test-support"))]
                if result.is_ok()
                    && let Some(observer) = observer
                {
                    // A dispatch can return without claiming an unadmitted push.
                    // Read once after it returns; only a committed acceptance is a fence.
                    let record = observed
                        .0
                        .lock()
                        .await
                        .read_delivery::<
                            collaboration_protocol::SessionRef,
                            collaboration_protocol::CodexGeneration,
                            serde_json::Value,
                        >(&observed.1)
                        .await?;
                    if record.status == agent_automation::DeliveryStatus::Accepted {
                        let _ = observer.send(observed.1);
                    }
                }
                result
            });
        }
        Ok(())
    }
    async fn evaluate_due(&self, now_ms: i64) -> Result<(), StorageError> {
        let ids = self.store.lock().await.due_wakeup_ids(now_ms, 100).await?;
        for id in ids {
            self.store
                .lock()
                .await
                .evaluate_wakeup::<SavedMessage>(
                    &id,
                    now_ms,
                    crate::wakeup_delivery_sender::build_wake_push_draft,
                )
                .await?;
        }
        Ok(())
    }
}
