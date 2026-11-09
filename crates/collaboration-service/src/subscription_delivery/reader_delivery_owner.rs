use router_clock::RuntimeClock;
#[path = "reader_delivery_actions.rs"]
mod reader_delivery_actions;
#[path = "reader_direct_messages.rs"]
mod reader_direct_messages;
#[path = "reader_follow_allocations.rs"]
mod reader_follow_allocations;
#[path = "reader_membership_refresh.rs"]
mod reader_membership_refresh;
#[path = "reader_native_mail.rs"]
mod reader_native_mail;
#[path = "reader_poll_mail.rs"]
mod reader_poll_mail;
pub(super) use reader_poll_mail::poll_unsent;
#[path = "reader_wait_handoff.rs"]
mod reader_wait_handoff;
use super::SubscriptionWindowStore;
use super::reader_scan_scope::ReaderScanScope;
#[cfg(test)]
use super::subscription_service::OwnerObservation;
use super::subscription_windows::{ProjectSubscriptionRecord, ReaderSubscriptionSnapshot};
use super::{
    BoardAvailability,
    subscription_facts::{RootSubscriptionFacts, root_facts, wall_deadline},
    subscription_push::{SubscriptionPushStore, target_session},
    subscription_wait::{PollWaiter, SubscriptionWaitFilter, SubscriptionWaitResult},
};
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, PushId, PushRecord};
use message_board::{
    BoardError, EndReason, Identity, ProjectId, SubscriptionBatch, SubscriptionBatchSettlement,
    SubscriptionDeliveryOutcome, SubscriptionMode, SubscriptionScope, ThreadId,
    ThreadSubscriptionRecord,
};
use router_clock::ClockInstant;
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

type DirectMessageReply = oneshot::Sender<Result<PushRecord, BoardError>>;
type MailAttemptReply = oneshot::Sender<collaboration_contracts::NativeDeliveryResult>;

pub(super) enum MailAttemptCheck {
    CarrierOnly,
    FrozenFollow {
        project: message_board::ProjectId,
        reference: fleet_sync_protocol::DeliveryItemRef,
    },
}
/// Answered by the owner's next pass: after its native window scan, or with the error
/// that ended the pass before that scan could complete.
type ReconcileReply = oneshot::Sender<Result<ReaderSubscriptionSnapshot, BoardError>>;

#[derive(Clone, Copy)]
pub(super) enum ReaderReconcilePurpose {
    Refresh,
    Restore,
    NativeMail,
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
    MailAttempt {
        kind: collaboration_protocol::PushKind,
        request: Box<collaboration_contracts::layer_zero::DeliveryRequest>,
        check: MailAttemptCheck,
        reply: MailAttemptReply,
    },
    PollMailAttempt {
        item: Box<fleet_sync_protocol::DeliveryItem<collaboration_contracts::RouterDeliveryTypes>>,
        reply: MailAttemptReply,
    },
    Wait {
        filter: SubscriptionWaitFilter,
        deadline: ClockInstant,
        maximum_root_notice_bytes: usize,
        reply: oneshot::Sender<Result<Option<SubscriptionWaitResult>, BoardError>>,
    },
}

/// The step of an owner pass whose failure armed the retry.
#[derive(Clone, Copy)]
enum FailedPassStage {
    MembershipExpiry,
    DirectMessages,
    SubscriptionRecords,
    SubscriptionPush,
    WaitSelection,
}

impl FailedPassStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::MembershipExpiry => "membership_expiry",
            Self::DirectMessages => "direct_messages",
            Self::SubscriptionRecords => "subscription_records",
            Self::SubscriptionPush => "subscription_push",
            Self::WaitSelection => "wait_selection",
        }
    }
}

pub(super) struct ReaderDeliveryOwnerProps {
    #[cfg(test)]
    pub poll_handover_probe: super::subscription_service::PollHandoverProbeSlot,
    pub native_mail: super::subscription_service::NativeMailReadiness,
    pub follow_admission: super::subscription_service::FollowAdmissionSlot,
    pub reader: Identity,
    pub initial_scope: ReaderScanScope,
    pub board_availability: BoardAvailability,
    pub push: Arc<SubscriptionPushStore>,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub clock: Arc<dyn RuntimeClock>,
    pub shutdown: CancellationToken,
    pub commands: mpsc::Receiver<ReaderDeliveryCommand>,
    pub activity: Option<broadcast::Receiver<ProjectId>>,
    pub held_mail: super::held_mail_signals::HeldMailWatch,
    #[cfg(test)]
    pub observations: broadcast::Sender<OwnerObservation>,
}

pub(super) struct ReaderDeliveryOwner {
    #[cfg(test)]
    poll_handover_probe: super::subscription_service::PollHandoverProbeSlot,
    native_mail: super::subscription_service::NativeMailReadiness,
    follow_admission: super::subscription_service::FollowAdmissionSlot,
    /// Held native items whose target was checked since the last signal it may have loaded.
    native_checked: std::collections::HashSet<PushId>,
    /// Signals that the target of held mail may have loaded.
    held_mail: super::held_mail_signals::HeldMailWatch,
    membership_expiry_deadline: Option<ClockInstant>,
    reader: Identity,
    board_availability: BoardAvailability,
    push: Arc<SubscriptionPushStore>,
    presence: Arc<dyn TargetPresenceProbe>,
    clock: Arc<dyn RuntimeClock>,
    shutdown: CancellationToken,
    commands: mpsc::Receiver<ReaderDeliveryCommand>,
    activity: Option<broadcast::Receiver<ProjectId>>,
    waiters: VecDeque<PollWaiter>,
    prior_records: ReaderSubscriptionSnapshot,
    pending_scope: Option<ReaderScanScope>,
    pending_restoration: Option<ReaderScanScope>,
    pass_projects: Vec<ProjectId>,
    pass_preventions: Vec<crate::notification_publisher::GenerationPreventedFollow>,
    pending_waiters: VecDeque<PollWaiter>,
    pending_ended_push_cleanup: Option<String>,
    /// When a failed clean-up of ended subscriptions is next retried, on its own backoff.
    held_cleanup_retry_at: Option<ClockInstant>,
    held_cleanup_backoff: collaboration_runtime::FailedPassBackoff,
    rescan: bool,
    reconcile_replies: Vec<ReconcileReply>,
    /// Held DMs whose target was checked since the last signal it may have loaded.
    dm_checked: std::collections::HashSet<PushId>,
    dm_replies: HashMap<PushId, Vec<DirectMessageReply>>,
    /// A failed pass retries on this backoff; the next pass that completes clears it.
    failed_pass_backoff: collaboration_runtime::FailedPassBackoff,
    #[cfg(test)]
    barriers: Vec<oneshot::Sender<()>>,
    #[cfg(test)]
    observations: broadcast::Sender<OwnerObservation>,
}

impl ReaderDeliveryOwner {
    pub(super) fn new(props: ReaderDeliveryOwnerProps) -> Self {
        let ReaderDeliveryOwnerProps {
            #[cfg(test)]
            poll_handover_probe,
            native_mail,
            follow_admission,
            reader,
            initial_scope,
            board_availability,
            push,
            presence,
            clock,
            shutdown,
            commands,
            activity,
            held_mail,
            #[cfg(test)]
            observations,
        } = props;
        Self {
            #[cfg(test)]
            poll_handover_probe,
            native_mail,
            follow_admission,
            native_checked: std::collections::HashSet::new(),
            held_mail,
            membership_expiry_deadline: None,
            reader,
            board_availability,
            push,
            presence,
            clock,
            shutdown,
            commands,
            activity,
            waiters: VecDeque::new(),
            prior_records: ReaderSubscriptionSnapshot::default(),
            pending_scope: Some(initial_scope),
            pending_restoration: None,
            pass_projects: Vec::new(),
            pass_preventions: Vec::new(),
            pending_waiters: VecDeque::new(),
            pending_ended_push_cleanup: None,
            held_cleanup_retry_at: None,
            held_cleanup_backoff: collaboration_runtime::FailedPassBackoff::default(),
            rescan: true,
            reconcile_replies: Vec::new(),
            dm_checked: std::collections::HashSet::new(),
            dm_replies: HashMap::new(),
            failed_pass_backoff: collaboration_runtime::FailedPassBackoff::default(),
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
            if let Err(error) = self.expire_due_follow_memberships().await {
                let retry = self.failed_pass_retry(FailedPassStage::MembershipExpiry, &error);
                self.answer_reconcile_requests(&Err(error));
                self.refuse_expired_pending_waiters();
                if !self.wait_for_input(Some(retry)).await {
                    break;
                }
                continue;
            }
            self.try_pending_held_push_cleanup().await;
            let pending_dms = match self.deliver_direct_messages().await {
                Ok(pending) => pending,
                Err(error) => {
                    let retry = self.failed_pass_retry(FailedPassStage::DirectMessages, &error);
                    self.answer_reconcile_requests(&Err(error));
                    self.refuse_expired_pending_waiters();
                    if !self.wait_for_input(Some(retry)).await {
                        break;
                    }
                    continue;
                }
            };
            // Poll eligibility is reconsidered by the existing rescan signals.
            // A callback returning Held must not create its own retry loop.
            let reconsider_poll = self.rescan;
            let current = match self.refresh_subscription_records().await {
                Ok(records) => records,
                Err(error) => {
                    let retry =
                        self.failed_pass_retry(FailedPassStage::SubscriptionRecords, &error);
                    self.answer_reconcile_requests(&Err(error));
                    self.refuse_expired_pending_waiters();
                    if !self.wait_for_input(Some(retry)).await {
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
            self.activate_pending_waiters(&current.records).await;
            if reconsider_poll && let Err(error) = self.resume_poll_waiters().await {
                tracing::warn!(%error, "Poll readiness discovery failed");
            }
            let now = self.clock.utc_now();
            #[cfg(test)]
            self.observe(OwnerObservation::RowsLoaded(records.len()));
            self.expiry_notices(&records).await;
            self.prior_records = current;
            if records.is_empty()
                && self.waiters.is_empty()
                && !pending_dms
                && self.pending_ended_push_cleanup.is_none()
            {
                // Close admission, then finish every accepted carrier call once.
                // Its effect cannot safely be requested again after a lost response.
                self.commands.close();
                while let Some(command) = self.commands.recv().await {
                    if matches!(
                        &command,
                        ReaderDeliveryCommand::MailAttempt { .. }
                            | ReaderDeliveryCommand::PollMailAttempt { .. }
                    ) {
                        self.handle_command(command).await;
                    }
                }
                break;
            }
            if self.complete_drains(&records) {
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
                        .due_project_subscription_roots(
                            &self.reader,
                            now,
                            &self.pass_projects,
                            &self.pass_preventions,
                        )
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
                    let retry = self.failed_pass_retry(FailedPassStage::SubscriptionPush, &error);
                    if !self.wait_for_input(Some(retry)).await {
                        break;
                    }
                    continue;
                }
            }
            match self.hand_to_waiter(&due, &facts).await {
                Ok(true) => continue,
                Ok(false) => {}
                Err(error) => {
                    let retry = self.failed_pass_retry(FailedPassStage::WaitSelection, &error);
                    if !self.wait_for_input(Some(retry)).await {
                        break;
                    }
                    continue;
                }
            }
            if self.failed_pass_backoff.record_success() {
                tracing::info!(
                    subscription_owner_retry = "recovered",
                    "subscription owner pass recovered"
                );
            }
            // Held mail adds no deadline: its target's load is a signal.
            let deadline = self.next_deadline(&records, &facts);
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

    fn refuse_expired_pending_waiters(&mut self) {
        let mut retained = VecDeque::new();
        while let Some(waiter) = self.pending_waiters.pop_front() {
            if waiter.reply.is_closed() {
                continue;
            }
            if waiter.deadline <= self.clock.monotonic_now() {
                let mut error = BoardError::board_unavailable();
                error.message =
                    "Subscription scan did not succeed before the wait deadline. Retry later."
                        .to_owned();
                let _ = waiter.reply.send(Err(error));
            } else {
                retained.push_back(waiter);
            }
        }
        self.pending_waiters = retained;
    }

    fn next_deadline(
        &self,
        records: &[ThreadSubscriptionRecord],
        facts: &HashMap<ThreadId, RootSubscriptionFacts>,
    ) -> Option<ClockInstant> {
        let now = self.clock.utc_now();
        let monotonic = self.clock.monotonic_now();
        let mut next_deadline = self
            .waiters
            .iter()
            .map(|waiter| waiter.deadline)
            .chain(self.membership_expiry_deadline)
            .chain(
                records
                    .iter()
                    .map(|record| wall_deadline(record.expires_at(), now, monotonic)),
            )
            .min();
        if self.pending_ended_push_cleanup.is_some()
            && let Some(cleanup_deadline) = self.held_cleanup_retry_at
        {
            next_deadline = Some(
                next_deadline.map_or(cleanup_deadline, |current| current.min(cleanup_deadline)),
            );
        }
        let mut include_deadline = |deadline: ClockInstant| {
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

    async fn wait_for_input(&mut self, deadline: Option<ClockInstant>) -> bool {
        let deadline = deadline
            .into_iter()
            .chain(
                self.pending_waiters
                    .iter()
                    .filter(|waiter| !waiter.reply.is_closed())
                    .map(|waiter| waiter.deadline),
            )
            .min();
        #[cfg(test)]
        for barrier in self.barriers.drain(..) {
            let _ = barrier.send(());
        }
        #[cfg(test)]
        self.observe(OwnerObservation::Sleeping(deadline));
        // Only a reader holding mail wakes on the signals that its target may have loaded.
        let holding = self.holds_mail();
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
            () = self.held_mail.changed(), if holding => {
                if let Ok(target) = target_session(&self.reader) {
                    let owed = self.held_mail.take_owed_check(&target).await;
                    if owed {
                        self.dm_checked.clear();
                        self.native_checked.clear();
                    }
                    #[cfg(test)]
                    self.observe(OwnerObservation::HeldMailSignalTaken { owed });
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
                if !matches!(purpose, ReaderReconcilePurpose::NativeMail) {
                    self.native_checked.clear();
                }
                // Acceptance is not reconciliation: the reply waits for the scan it requested.
                self.request_scan(scope);
                self.reconcile_replies.push(reply);
                #[cfg(test)]
                self.observe(OwnerObservation::ReconcileRequested);
            }
            ReaderDeliveryCommand::DmQueued { push_id, reply } => {
                self.dm_checked.remove(&push_id);
                self.dm_replies.entry(push_id).or_default().push(reply);
            }
            ReaderDeliveryCommand::MailAttempt {
                kind,
                request,
                check,
                reply,
            } => {
                if let MailAttemptCheck::FrozenFollow { project, reference } = check {
                    let publisher = self
                        .follow_admission
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .as_ref()
                        .and_then(|port| port.publisher.upgrade());
                    let decision = match publisher {
                        Some(publisher) => {
                            publisher
                                .before_local_follow_injection(
                                    &project,
                                    &reference,
                                    &request.target,
                                )
                                .await
                        }
                        None => Err(BoardError::board_unavailable()),
                    };
                    if !matches!(decision, Ok(false)) {
                        if let Err(error) = decision {
                            tracing::warn!(%error, "local follow cancellation check delayed");
                        }
                        let _ = reply.send(collaboration_contracts::NativeDeliveryResult::not_submitted(
                            "local follow injection waits for its cancellation check",
                            true,
                            collaboration_contracts::UnsentCause::Holdable {
                                otherwise: collaboration_contracts::NativeRefusalCode::CarrierUnavailable,
                            },
                        ));
                        return;
                    }
                }
                let receipt = self
                    .push
                    .dispatch_mail_attempt(kind, *request)
                    .await
                    .unwrap_or_else(|_| collaboration_contracts::NativeDeliveryResult::unknown());
                let _ = reply.send(receipt);
            }
            ReaderDeliveryCommand::PollMailAttempt { item, reply } => {
                let outcome = self.hand_over_poll_mail(item).await;
                let _ = reply.send(outcome);
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
                    poll_filter_mismatches: std::collections::HashSet::new(),
                    poll_in_flight: None,
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

    /// Whether mail this reader checked is still held, waiting for a load signal.
    pub(super) fn holds_mail(&self) -> bool {
        !self.dm_checked.is_empty() || !self.native_checked.is_empty()
    }

    /// Arms the failed pass's retry on the backoff, logging only the failure that starts a run.
    fn failed_pass_retry(&mut self, stage: FailedPassStage, error: &BoardError) -> ClockInstant {
        let retry = self.failed_pass_backoff.record_failure();
        #[cfg(test)]
        self.observe(OwnerObservation::PassFailed(stage.as_str()));
        if retry.starts_run {
            tracing::warn!(
                %error,
                stage = stage.as_str(),
                subscription_owner_retry = "failing",
                "subscription owner pass failed; retrying with backoff"
            );
        }
        let now = self.clock.monotonic_now();
        now.checked_add(retry.delay).unwrap_or(now)
    }
    #[cfg(test)]
    fn observe(&self, observation: OwnerObservation) {
        let _ = self.observations.send(observation);
    }
}
