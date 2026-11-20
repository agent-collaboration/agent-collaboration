use router_clock::RuntimeClock;
#[path = "reader_direct_messages.rs"]
mod reader_direct_messages;
#[path = "reader_native_mail.rs"]
mod reader_native_mail;
use super::subscription_push::{SubscriptionPushStore, target_session};
#[cfg(test)]
use super::subscription_service::OwnerObservation;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::TargetPresence;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_protocol::{PushId, PushRecord};
use message_board::{BoardError, Identity};
use router_clock::ClockInstant;
use std::{collections::HashMap, sync::Arc};
#[cfg(test)]
use tokio::sync::broadcast;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

type DirectMessageReply = oneshot::Sender<Result<PushRecord, BoardError>>;
type MailAttemptReply = oneshot::Sender<collaboration_contracts::NativeDeliveryResult>;

pub(super) enum ReaderDeliveryCommand {
    DmQueued {
        push_id: PushId,
        reply: DirectMessageReply,
    },
    MailAttempt {
        kind: collaboration_protocol::PushKind,
        request: Box<collaboration_contracts::layer_zero::DeliveryRequest>,
        reply: MailAttemptReply,
    },
    NativeMailChanged {
        observed: oneshot::Sender<()>,
    },
}

pub(super) struct ReaderDeliveryOwnerProps {
    pub native_mail: super::subscription_service::NativeMailReadiness,
    pub reader: Identity,
    pub push: Arc<SubscriptionPushStore>,
    pub presence: Arc<dyn TargetPresenceProbe>,
    pub clock: Arc<dyn RuntimeClock>,
    pub shutdown: CancellationToken,
    pub commands: mpsc::Receiver<ReaderDeliveryCommand>,
    pub held_mail: super::held_mail_signals::HeldMailWatch,
    #[cfg(test)]
    pub observations: broadcast::Sender<OwnerObservation>,
}

pub(super) struct ReaderDeliveryOwner {
    native_mail: super::subscription_service::NativeMailReadiness,
    /// Held native items whose target was checked since the last signal it may have loaded.
    native_checked: std::collections::HashSet<PushId>,
    /// Signals that the target of held mail may have loaded.
    held_mail: super::held_mail_signals::HeldMailWatch,
    reader: Identity,
    push: Arc<SubscriptionPushStore>,
    presence: Arc<dyn TargetPresenceProbe>,
    clock: Arc<dyn RuntimeClock>,
    shutdown: CancellationToken,
    commands: mpsc::Receiver<ReaderDeliveryCommand>,
    /// Held DMs whose target was checked since the last signal it may have loaded.
    dm_checked: std::collections::HashSet<PushId>,
    dm_replies: HashMap<PushId, Vec<DirectMessageReply>>,
    native_mail_observers: Vec<oneshot::Sender<()>>,
    /// A failed pass retries on this backoff; the next pass that completes clears it.
    failed_pass_backoff: collaboration_runtime::FailedPassBackoff,
    #[cfg(test)]
    observations: broadcast::Sender<OwnerObservation>,
}

impl ReaderDeliveryOwner {
    pub(super) fn new(props: ReaderDeliveryOwnerProps) -> Self {
        let ReaderDeliveryOwnerProps {
            native_mail,
            reader,
            push,
            presence,
            clock,
            shutdown,
            commands,
            held_mail,
            #[cfg(test)]
            observations,
        } = props;
        Self {
            native_mail,
            native_checked: std::collections::HashSet::new(),
            held_mail,
            reader,
            push,
            presence,
            clock,
            shutdown,
            commands,
            dm_checked: std::collections::HashSet::new(),
            dm_replies: HashMap::new(),
            native_mail_observers: Vec::new(),
            failed_pass_backoff: collaboration_runtime::FailedPassBackoff::default(),
            #[cfg(test)]
            observations,
        }
    }

    pub(super) async fn run(mut self) {
        #[cfg(test)]
        self.observe(OwnerObservation::Started);
        while !self.shutdown.is_cancelled() {
            while let Ok(command) = self.commands.try_recv() {
                self.handle_command(command).await;
            }
            let pending_dms = match self.deliver_direct_messages().await {
                Ok(pending) => pending,
                Err(error) => {
                    let retry = self.failed_pass_retry(&error);
                    if !self.wait_for_input(Some(retry)).await {
                        break;
                    }
                    continue;
                }
            };
            for observed in self.native_mail_observers.drain(..) {
                let _ = observed.send(());
            }
            if !pending_dms {
                // Close admission, then finish every accepted carrier call once.
                // Its effect cannot safely be requested again after a lost response.
                self.commands.close();
                while let Some(command) = self.commands.recv().await {
                    if matches!(&command, ReaderDeliveryCommand::MailAttempt { .. }) {
                        self.handle_command(command).await;
                    }
                }
                break;
            }
            if self.failed_pass_backoff.record_success() {
                tracing::info!(
                    subscription_owner_retry = "recovered",
                    "subscription owner pass recovered"
                );
            }
            // Held mail adds no deadline: its target's load is a signal.
            if !self.wait_for_input(None).await {
                break;
            }
        }
        #[cfg(test)]
        self.observe(OwnerObservation::Stopped);
    }

    async fn wait_for_input(&mut self, deadline: Option<ClockInstant>) -> bool {
        #[cfg(test)]
        self.observe(OwnerObservation::Sleeping);
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
            () = self.held_mail.changed(), if holding => {
                if let Ok(target) = target_session(&self.reader) {
                    let owed = self.held_mail.take_owed_check(&target).await;
                    if owed {
                        self.dm_checked.clear();
                        self.native_checked.clear();
                    }
                    #[cfg(test)]
                    self.observe(OwnerObservation::HeldMailSignalTaken);
                }
                true
            },
            () = timer => true,
        }
    }

    async fn handle_command(&mut self, command: ReaderDeliveryCommand) {
        match command {
            ReaderDeliveryCommand::DmQueued { push_id, reply } => {
                self.dm_checked.remove(&push_id);
                self.dm_replies.entry(push_id).or_default().push(reply);
            }
            ReaderDeliveryCommand::MailAttempt {
                kind,
                request,
                reply,
            } => {
                let receipt = self
                    .push
                    .dispatch_mail_attempt(kind, *request)
                    .await
                    .unwrap_or_else(|_| collaboration_contracts::NativeDeliveryResult::unknown());
                let _ = reply.send(receipt);
            }
            ReaderDeliveryCommand::NativeMailChanged { observed } => {
                self.native_mail_observers.push(observed);
            }
        }
    }

    /// Whether mail this reader checked is still held, waiting for a load signal.
    pub(super) fn holds_mail(&self) -> bool {
        !self.dm_checked.is_empty() || !self.native_checked.is_empty()
    }

    /// Arms the failed pass's retry on the backoff, logging only the failure that starts a run.
    fn failed_pass_retry(&mut self, error: &BoardError) -> ClockInstant {
        let retry = self.failed_pass_backoff.record_failure();
        #[cfg(test)]
        self.observe(OwnerObservation::PassFailed);
        if retry.starts_run {
            tracing::warn!(
                %error,
                stage = "direct_messages",
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
