#[path = "reader_delivery_actions.rs"]
mod reader_delivery_actions;
#[path = "reader_direct_messages.rs"]
mod reader_direct_messages;
#[path = "reader_membership_refresh.rs"]
mod reader_membership_refresh;
#[path = "reader_wait_handoff.rs"]
mod reader_wait_handoff;
use super::SubscriptionWindowStore;
use super::reader_scan_scope::ReaderScanScope;
#[cfg(test)]
use super::subscription_service::OwnerObservation;
use super::subscription_windows::{ProjectSubscriptionRecord, ReaderSubscriptionSnapshot};
use super::{
    BoardAvailability, SubscriptionClock,
    subscription_facts::{
        PRESENCE_INTERVAL, RootSubscriptionFacts, STORAGE_RETRY_INTERVAL, delivery_retry_delay,
        root_facts, wall_deadline,
    },
    subscription_push::{SubscriptionPushStore, target_session},
    subscription_wait::{PollWaiter, SubscriptionWaitFilter, SubscriptionWaitResult},
};
use crate::{LoadPolicy, TargetPresence, TargetPresenceProbe};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId, PushRecord};
use message_board::{
    BoardError, EndReason, Identity, ProjectId, SubscriptionBatch, SubscriptionBatchSettlement,
    SubscriptionDeliveryOutcome, SubscriptionMode, SubscriptionScope, ThreadId,
    ThreadSubscriptionRecord, WhenIdle,
};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

type DirectMessageReply = oneshot::Sender<Result<PushRecord, BoardError>>;
/// Answered by the owner's next pass: after its native window scan, or with the error
/// that ended the pass before that scan could complete.
type ReconcileReply = oneshot::Sender<Result<ReaderSubscriptionSnapshot, BoardError>>;

#[derive(Clone, Copy)]
pub(super) enum ReaderReconcilePurpose {
    Refresh,
    Restore,
}

pub(super) enum ReaderDeliveryCommand {
    #[cfg(test)]
    Barrier(oneshot::Sender<()>),
    Reconcile {
        scope: ReaderScanScope,
        purpose: ReaderReconcilePurpose,
        reply: ReconcileReply,
    },
    DmQueued {
        push_id: PushId,
        reply: DirectMessageReply,
    },
    Wait {
        filter: SubscriptionWaitFilter,
        deadline: Instant,
        maximum_root_notice_bytes: usize,
        reply: oneshot::Sender<Result<Option<SubscriptionWaitResult>, BoardError>>,
    },
}

pub(super) struct ReaderDeliveryOwnerProps {
    pub reader: Identity,
    pub initial_scope: ReaderScanScope,
    pub board_availability: BoardAvailability,
    pub push: Arc<SubscriptionPushStore>,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub clock: Arc<dyn SubscriptionClock>,
    pub shutdown: CancellationToken,
    pub commands: mpsc::Receiver<ReaderDeliveryCommand>,
    pub activity: Option<broadcast::Receiver<ProjectId>>,
    #[cfg(test)]
    pub observations: broadcast::Sender<OwnerObservation>,
}

#[derive(Default)]
struct PendingHeldPushCleanup {
    superseded: Option<(PushId, String)>,
    ended_reason: Option<String>,
}

impl PendingHeldPushCleanup {
    fn is_pending(&self) -> bool {
        self.superseded.is_some() || self.ended_reason.is_some()
    }
}

pub(super) struct ReaderDeliveryOwner {
    reader: Identity,
    board_availability: BoardAvailability,
    push: Arc<SubscriptionPushStore>,
    presence: Arc<dyn TargetPresenceProbe>,
    clock: Arc<dyn SubscriptionClock>,
    shutdown: CancellationToken,
    commands: mpsc::Receiver<ReaderDeliveryCommand>,
    activity: Option<broadcast::Receiver<ProjectId>>,
    waiters: VecDeque<PollWaiter>,
    checked_presence: HashMap<ThreadId, Instant>,
    prior_records: ReaderSubscriptionSnapshot,
    pending_scope: Option<ReaderScanScope>,
    pending_restoration: Option<ReaderScanScope>,
    pass_projects: Vec<ProjectId>,
    pending_waiters: VecDeque<PollWaiter>,
    pending_held_cleanup: PendingHeldPushCleanup,
    rescan: bool,
    reconcile_replies: Vec<ReconcileReply>,
    dm_checked_presence: HashMap<PushId, Instant>,
    dm_replies: HashMap<PushId, Vec<DirectMessageReply>>,
    #[cfg(test)]
    barriers: Vec<oneshot::Sender<()>>,
    #[cfg(test)]
    observations: broadcast::Sender<OwnerObservation>,
}

impl ReaderDeliveryOwner {
    pub(super) fn new(props: ReaderDeliveryOwnerProps) -> Self {
        let ReaderDeliveryOwnerProps {
            reader,
            initial_scope,
            board_availability,
            push,
            presence,
            clock,
            shutdown,
            commands,
            activity,
            #[cfg(test)]
            observations,
        } = props;
        Self {
            reader,
            board_availability,
            push,
            presence,
            clock,
            shutdown,
            commands,
            activity,
            waiters: VecDeque::new(),
            checked_presence: HashMap::new(),
            prior_records: ReaderSubscriptionSnapshot::default(),
            pending_scope: Some(initial_scope),
            pending_restoration: None,
            pass_projects: Vec::new(),
            pending_waiters: VecDeque::new(),
            pending_held_cleanup: PendingHeldPushCleanup::default(),
            rescan: true,
            reconcile_replies: Vec::new(),
            dm_checked_presence: HashMap::new(),
            dm_replies: HashMap::new(),
            #[cfg(test)]
            barriers: Vec::new(),
            #[cfg(test)]
            observations,
        }
    }

    pub(super) async fn run(mut self) {
        #[cfg(test)]
        self.observe(OwnerObservation::Started);
        while !self.shutdown.is_cancelled() {
            self.expire_waiters();
            while let Ok(command) = self.commands.try_recv() {
                self.handle_command(command).await;
            }
            self.try_pending_held_push_cleanup().await;
            let pending_dms = match self.deliver_direct_messages().await {
                Ok(pending) => pending,
                Err(error) => {
                    tracing::warn!(%error, "direct-message reload or delivery failed");
                    self.answer_reconcile_requests(&Err(error));
                    if !self.wait_for_input(Some(self.retry_deadline())).await {
                        break;
                    }
                    continue;
                }
            };
            let current = match self.refresh_subscription_records().await {
                Ok(records) => records,
                Err(error) => {
                    self.answer_reconcile_requests(&Err(error));
                    if !self.wait_for_input(Some(self.retry_deadline())).await {
                        break;
                    }
                    continue;
                }
            };
            let records: Vec<_> = current
                .records
                .iter()
                .map(|row| row.record.clone())
                .collect();
            self.answer_reconcile_requests(&Ok(current.clone()));
            self.activate_pending_waiters(&current.records);
            let now = self.clock.now();
            #[cfg(test)]
            self.observe(OwnerObservation::RowsLoaded(records.len()));
            self.expiry_notices(&records).await;
            self.prior_records = current;
            if records.is_empty()
                && self.waiters.is_empty()
                && !pending_dms
                && !self.pending_held_cleanup.is_pending()
            {
                // Closing makes a concurrent sender retry with a new owner. Commands accepted
                // before this close lose their response and retry through the service.
                self.commands.close();
                break;
            }
            if self.complete_drains(&records).await {
                continue;
            }
            let facts = root_facts(&self.prior_records.records);
            #[cfg(test)]
            self.observe(OwnerObservation::SelectingDueRoots);
            let due_result = match &self.board_availability {
                BoardAvailability::Available(store) => {
                    store
                        .lock()
                        .await
                        .due_project_subscription_roots(&self.reader, now, &self.pass_projects)
                        .await
                }
                BoardAvailability::Unavailable => Ok(Vec::new()),
            };
            let due = match due_result {
                Ok(roots) => roots,
                Err(error) => {
                    tracing::warn!(%error, "subscription due selection failed");
                    Vec::new()
                }
            };
            #[cfg(test)]
            self.observe(OwnerObservation::DueRoots(due.len()));
            match self.deliver_due(&due, &facts).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, "subscription push failed before effect");
                    if !self.wait_for_input(Some(self.retry_deadline())).await {
                        break;
                    }
                    continue;
                }
            }
            match self.hand_to_waiter(&due, &facts).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, "subscription wait selection failed");
                    if !self.wait_for_input(Some(self.retry_deadline())).await {
                        break;
                    }
                    continue;
                }
            }
            let deadline = self.next_deadline(&records, &facts).await;
            let dm_deadline = self
                .dm_checked_presence
                .values()
                .filter_map(|checked| checked.checked_add(PRESENCE_INTERVAL))
                .min();
            let deadline = match (deadline, dm_deadline) {
                (Some(left), Some(right)) => Some(left.min(right)),
                (left, right) => left.or(right),
            };
            if !self.wait_for_input(deadline).await {
                break;
            }
        }
        while let Some(waiter) = self.waiters.pop_front() {
            let _ = waiter.reply.send(Ok(None));
        }
        #[cfg(test)]
        self.observe(OwnerObservation::Stopped);
    }

    fn expire_waiters(&mut self) {
        let mut retained = VecDeque::new();
        while let Some(waiter) = self.waiters.pop_front() {
            if waiter.reply.is_closed() {
                continue;
            }
            if waiter.deadline <= self.clock.monotonic_now() {
                let _ = waiter.reply.send(Ok(None));
            } else {
                retained.push_back(waiter);
            }
        }
        self.waiters = retained;
    }

    async fn next_deadline(
        &self,
        records: &[ThreadSubscriptionRecord],
        facts: &HashMap<ThreadId, RootSubscriptionFacts>,
    ) -> Option<Instant> {
        let now = self.clock.now();
        let monotonic = self.clock.monotonic_now();
        let mut next_deadline = self
            .waiters
            .iter()
            .map(|waiter| waiter.deadline)
            .chain(
                records
                    .iter()
                    .map(|record| wall_deadline(record.expires_at(), now, monotonic)),
            )
            .min();
        if self.pending_held_cleanup.is_pending() {
            let cleanup_deadline = monotonic
                .checked_add(PRESENCE_INTERVAL)
                .unwrap_or(monotonic);
            next_deadline = Some(
                next_deadline.map_or(cleanup_deadline, |current| current.min(cleanup_deadline)),
            );
        }
        let mut include_deadline = |deadline: Instant| {
            next_deadline = Some(next_deadline.map_or(deadline, |current| current.min(deadline)));
        };
        for (root, facts) in facts {
            if facts.mode == SubscriptionMode::Off {
                continue;
            }
            if facts.mode == SubscriptionMode::Poll {
                let mut covered = false;
                for waiter in &self.waiters {
                    covered |= waiter.filter.matches(root, &facts.project);
                }
                if !covered {
                    continue;
                }
            }
            if let Some(retry) = facts.next_retry_at {
                include_deadline(wall_deadline(retry, now, monotonic));
                continue;
            }
            if facts.held_since.is_some() {
                include_deadline(
                    self.checked_presence
                        .get(root)
                        .and_then(|checked| checked.checked_add(PRESENCE_INTERVAL))
                        .unwrap_or(monotonic),
                );
                continue;
            }
            let quiet = facts
                .last_arrival_at
                .checked_add_signed(chrono::Duration::seconds(
                    i64::try_from(facts.quiet_seconds).unwrap_or(i64::MAX),
                ));
            let cap = facts
                .opened_at
                .checked_add_signed(chrono::Duration::seconds(
                    i64::try_from(facts.cap_seconds).unwrap_or(i64::MAX),
                ));
            if let (Some(quiet), Some(cap)) = (quiet, cap) {
                include_deadline(wall_deadline(quiet.min(cap), now, monotonic));
            }
        }
        next_deadline
    }

    async fn wait_for_input(&mut self, deadline: Option<Instant>) -> bool {
        #[cfg(test)]
        for barrier in self.barriers.drain(..) {
            let _ = barrier.send(());
        }
        #[cfg(test)]
        self.observe(OwnerObservation::Sleeping(deadline));
        let clock = self.clock.clone();
        let timer = async {
            match deadline {
                Some(deadline) => clock.sleep_until(deadline).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            () = self.shutdown.cancelled() => false,
            command = self.commands.recv() => {
                match command {
                    Some(command) => { self.handle_command(command).await; true },
                    None => false,
                }
            },
            activity = async { match self.activity.as_mut() {
                Some(activity) => activity.recv().await,
                None => std::future::pending().await,
            } } => {
                match activity {
                    Ok(project) => self.request_scan(ReaderScanScope::project(project)),
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        self.request_scan(ReaderScanScope::All);
                        #[cfg(test)]
                        self.observe(OwnerObservation::ActivityLagged);
                    },
                    Err(broadcast::error::RecvError::Closed) => return false,
                }
                true
            },
            () = timer => true,
        }
    }

    async fn handle_command(&mut self, command: ReaderDeliveryCommand) {
        match command {
            #[cfg(test)]
            ReaderDeliveryCommand::Barrier(reply) => self.barriers.push(reply),
            ReaderDeliveryCommand::Reconcile {
                scope,
                purpose,
                reply,
            } => {
                if matches!(purpose, ReaderReconcilePurpose::Restore) {
                    match &mut self.pending_restoration {
                        Some(pending) => pending.merge(scope.clone()),
                        None => self.pending_restoration = Some(scope.clone()),
                    }
                }
                // Acceptance is not reconciliation: the reply waits for the scan it requested.
                self.request_scan(scope);
                self.reconcile_replies.push(reply);
                #[cfg(test)]
                self.observe(OwnerObservation::ReconcileRequested);
            }
            ReaderDeliveryCommand::DmQueued { push_id, reply } => {
                self.dm_checked_presence.remove(&push_id);
                self.dm_replies.entry(push_id).or_default().push(reply);
            }
            ReaderDeliveryCommand::Wait {
                filter,
                deadline,
                maximum_root_notice_bytes,
                reply,
            } => {
                self.request_scan(ReaderScanScope::All);
                self.pending_waiters.push_back(PollWaiter {
                    filter,
                    deadline,
                    maximum_root_notice_bytes,
                    reply,
                });
            }
        }
    }

    fn validate_poll_wait(
        &self,
        filter: &SubscriptionWaitFilter,
        records: &[ProjectSubscriptionRecord],
    ) -> Result<(), BoardError> {
        filter.covered_poll_scopes(records)?;
        Ok(())
    }

    fn answer_reconcile_requests(
        &mut self,
        outcome: &Result<ReaderSubscriptionSnapshot, BoardError>,
    ) {
        for reply in self.reconcile_replies.drain(..) {
            let _ = reply.send(outcome.clone());
        }
    }

    fn board_store(&self) -> Result<&Arc<Mutex<SubscriptionWindowStore>>, BoardError> {
        self.board_availability.require_store()
    }

    fn retry_deadline(&self) -> Instant {
        self.clock
            .monotonic_now()
            .checked_add(STORAGE_RETRY_INTERVAL)
            .unwrap_or_else(|| self.clock.monotonic_now())
    }
    #[cfg(test)]
    fn observe(&self, observation: OwnerObservation) {
        let _ = self.observations.send(observation);
    }
}
