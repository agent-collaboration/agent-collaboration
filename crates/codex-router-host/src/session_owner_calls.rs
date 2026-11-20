//! Retain caller payloads until the session owner settles them.

use crate::session_owner_delivery_evidence::OwnerDeliveryEvidence;
use acp_client_runtime::ProviderPromptContent;
use acp_client_runtime::session_owner_task::OwnerCompletion;
use collaboration_protocol::ProviderIdentity;
use provider_session_machines::{
    SessionGeneration,
    session_owner::{
        AuthorizationTarget, CancelDrainReport, DeliveryPlacement, DispatchAuthorization,
        EffectOutcome, OwnerEvent, OwnerInput, RemoteEffect, RestoreResult, RevokeReason,
        SessionOwnerReply,
    },
    session_turn_machine::TurnId,
};
use session_event_model::InputId;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, MutexGuard},
};
use tokio::sync::{Notify, oneshot};
use tokio_util::task::TaskTracker;
use uuid::Uuid;

#[derive(Default)]
pub struct SessionOwnerCalls {
    content: Mutex<BTreeMap<InputId, ProviderPromptContent>>,
    outstanding: Mutex<BTreeMap<InputId, OwnerInput>>,
    approvals: Mutex<BTreeMap<InputId, crate::ExternalProviderApprovalContext>>,
    acceptance: Mutex<BTreeMap<Uuid, oneshot::Sender<InputAcceptance>>>,
    delivery_evidence: Mutex<BTreeMap<Uuid, OwnerDeliveryEvidence>>,
    active_turns: Mutex<BTreeMap<TurnId, Uuid>>,
    steer_targets: Mutex<BTreeMap<Uuid, SteerTarget>>,
    replies: Mutex<BTreeMap<Uuid, oneshot::Sender<OwnedSettlement>>>,
    reply_changed: Notify,
    cancels: Mutex<BTreeMap<(SessionGeneration, TurnId), oneshot::Sender<CancelReceipt>>>,
    dependent_restores: Mutex<BTreeMap<Uuid, oneshot::Sender<RestoreResult>>>,
    idle_steers: Mutex<BTreeSet<Uuid>>,
}

pub(crate) type CancelReceipt = Result<CancelDrainReport, RevokeReason>;

struct SteerTarget {
    operation: Option<Uuid>,
    different_approver: Option<ProviderIdentity>,
}

pub struct OwnedSettlement {
    pub reply: SessionOwnerReply,
    pub saved: bool,
    pub idle_steer_summary: Option<&'static str>,
    pub(crate) turn_approver: Option<ProviderIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputAcceptance {
    QueuePlanChanged,
    Queued,
    Started,
    ProviderStarted,
    Steered { joined_operation: Option<Uuid> },
    Unsent { reason: Option<RevokeReason> },
    Unknown,
}

impl SessionOwnerCalls {
    pub(crate) fn expect_delivery_evidence(
        &self,
        operation: Uuid,
        evidence: OwnerDeliveryEvidence,
    ) {
        lock(&self.delivery_evidence).insert(operation, evidence);
    }

    pub(crate) fn offer_delivery_evidence(
        &self,
        placement: DeliveryPlacement,
        completion: OwnerCompletion,
        tasks: &TaskTracker,
    ) {
        if let Some(evidence) = lock(&self.delivery_evidence).get_mut(&placement.input.operation) {
            evidence.offer(placement, completion, tasks);
        } else {
            completion.send(OwnerEvent::DeliveryEvidence {
                placement,
                outcome: EffectOutcome::Failed,
            });
        }
    }

    pub(crate) fn content(&self, input: &InputId) -> Option<ProviderPromptContent> {
        lock(&self.content).get(input).cloned()
    }

    pub(crate) fn take_approval(
        &self,
        input: &InputId,
    ) -> Option<crate::ExternalProviderApprovalContext> {
        lock(&self.approvals).remove(input)
    }

    pub(crate) fn remember_approval(
        &self,
        input: InputId,
        context: crate::ExternalProviderApprovalContext,
    ) {
        lock(&self.approvals).insert(input, context);
    }

    pub(crate) fn deliver_restore(&self, operation: Uuid, result: RestoreResult) {
        if let Some(caller) = lock(&self.dependent_restores).remove(&operation) {
            let _caller_gone = caller.send(result);
        }
    }

    pub(crate) fn record_idle_steer(&self, operation: Uuid) {
        lock(&self.idle_steers).insert(operation);
    }
    pub fn insert_content(&self, input: &OwnerInput, content: ProviderPromptContent) {
        lock(&self.outstanding).insert(input.input.clone(), input.clone());
        lock(&self.content).insert(input.input.clone(), content);
    }

    pub(crate) fn outstanding_inputs(&self) -> Vec<OwnerInput> {
        lock(&self.outstanding).values().cloned().collect()
    }

    pub fn release_input(&self, input: &InputId) {
        lock(&self.content).remove(input);
        lock(&self.outstanding).remove(input);
        lock(&self.approvals).remove(input);
    }

    pub fn release_operation(&self, operation: Uuid) {
        lock(&self.replies).remove(&operation);
        self.reply_changed.notify_waiters();
        lock(&self.dependent_restores).remove(&operation);
        lock(&self.idle_steers).remove(&operation);
        lock(&self.acceptance).remove(&operation);
        lock(&self.delivery_evidence).remove(&operation);
        lock(&self.steer_targets).remove(&operation);
    }

    pub fn stop(&self) {
        lock(&self.replies).clear();
        lock(&self.cancels).clear();
        self.reply_changed.notify_waiters();
        lock(&self.dependent_restores).clear();
        lock(&self.content).clear();
        lock(&self.outstanding).clear();
        lock(&self.approvals).clear();
        lock(&self.idle_steers).clear();
        lock(&self.acceptance).clear();
        lock(&self.delivery_evidence).clear();
        lock(&self.active_turns).clear();
        lock(&self.steer_targets).clear();
    }

    pub(crate) async fn wait_for_replies(&self) {
        loop {
            let changed = self.reply_changed.notified();
            if lock(&self.replies).is_empty() && lock(&self.cancels).is_empty() {
                return;
            }
            changed.await;
        }
    }

    pub(crate) fn expect_cancel(
        &self,
        generation: SessionGeneration,
        turn: TurnId,
    ) -> Option<oneshot::Receiver<CancelReceipt>> {
        let mut cancels = lock(&self.cancels);
        let std::collections::btree_map::Entry::Vacant(waiter) = cancels.entry((generation, turn))
        else {
            return None;
        };
        let (send, receive) = oneshot::channel();
        waiter.insert(send);
        Some(receive)
    }

    pub(crate) fn forget_cancel(&self, generation: SessionGeneration, turn: TurnId) {
        lock(&self.cancels).remove(&(generation, turn));
        self.reply_changed.notify_waiters();
    }

    pub(crate) fn settle_cancel(
        &self,
        authorization: &DispatchAuthorization,
        receipt: CancelReceipt,
    ) {
        let AuthorizationTarget::ExistingTurn { turn } = authorization.target else {
            return;
        };
        if let Some(waiter) = lock(&self.cancels).remove(&(authorization.run, turn)) {
            let _caller_gone = waiter.send(receipt);
            self.reply_changed.notify_waiters();
        }
    }

    pub fn expect_settlement(&self, operation: Uuid) -> oneshot::Receiver<OwnedSettlement> {
        let (send, receive) = oneshot::channel();
        lock(&self.replies).insert(operation, send);
        receive
    }

    pub(crate) fn settlement_requested(&self, operation: Uuid) -> bool {
        lock(&self.replies)
            .get(&operation)
            .is_some_and(|reply| !reply.is_closed())
    }

    pub(crate) fn expect_acceptance(&self, operation: Uuid) -> oneshot::Receiver<InputAcceptance> {
        let (send, receive) = oneshot::channel();
        lock(&self.acceptance).insert(operation, send);
        receive
    }

    pub(crate) fn accepted(&self, operation: Uuid, acceptance: InputAcceptance) {
        if let Some(waiter) = lock(&self.acceptance).remove(&operation) {
            let _caller_gone = waiter.send(acceptance);
        }
    }

    pub(crate) fn prompt_taken(&self, input: OwnerInput) {
        self.record_active_turn(&input);
        self.accepted(input.operation, InputAcceptance::Started);
    }

    pub(crate) fn record_active_turn(&self, input: &OwnerInput) {
        lock(&self.active_turns).insert(input.turn, input.operation);
    }

    pub(crate) fn end_turn(&self, turn: TurnId) {
        lock(&self.active_turns).remove(&turn);
    }

    pub(crate) fn capture_steer_target(
        &self,
        input: &OwnerInput,
        approver: Option<&ProviderIdentity>,
    ) {
        let operation = lock(&self.active_turns).get(&input.turn).copied();
        let different_approver = approver
            .filter(|turn_approver| {
                lock(&self.approvals)
                    .get(&input.input)
                    .is_some_and(|asked| &asked.approver != *turn_approver)
            })
            .cloned();
        lock(&self.steer_targets).insert(
            input.operation,
            SteerTarget {
                operation,
                different_approver,
            },
        );
    }

    pub fn expect_restore(&self, operation: Uuid) -> oneshot::Receiver<RestoreResult> {
        let (send, receive) = oneshot::channel();
        lock(&self.dependent_restores).insert(operation, send);
        receive
    }

    pub(crate) fn settle(&self, reply: SessionOwnerReply, saved: bool) {
        let operation = match &reply {
            SessionOwnerReply::RestoreFinished(result) => result.attempt.attempt.0,
            SessionOwnerReply::CloseFinished { run, .. } => run.attempt.0,
            SessionOwnerReply::InputFinished { input, .. }
            | SessionOwnerReply::PromptCompleted { input, .. }
            | SessionOwnerReply::PromptFailed { input, .. }
            | SessionOwnerReply::PromptCancelUnconfirmed { input }
            | SessionOwnerReply::SteerStarted { input, .. }
            | SessionOwnerReply::SteerInvalidReply { input }
            | SessionOwnerReply::InputUnsent { input, .. } => {
                self.release_input(&input.input);
                input.operation
            }
            SessionOwnerReply::SettingFinished { setting, .. }
            | SessionOwnerReply::SettingKept { setting }
            | SessionOwnerReply::SettingNotSent { setting, .. }
            | SessionOwnerReply::ModelFastFinished { setting, .. } => setting.change.attempt.0,
        };
        let idle_steer_summary = lock(&self.idle_steers)
            .remove(&operation)
            .then_some("no turn was running; sent as a new prompt");
        lock(&self.delivery_evidence).remove(&operation);
        let joined = lock(&self.steer_targets).remove(&operation);
        let joined_operation = joined.as_ref().and_then(|target| target.operation);
        let acceptance = match &reply {
            SessionOwnerReply::SteerStarted { .. } => InputAcceptance::ProviderStarted,
            SessionOwnerReply::InputFinished {
                remote: RemoteEffect::Applied,
                ..
            } => InputAcceptance::Steered { joined_operation },
            SessionOwnerReply::InputUnsent { reason, .. } => InputAcceptance::Unsent {
                reason: Some(*reason),
            },
            SessionOwnerReply::InputFinished {
                remote: RemoteEffect::None,
                ..
            }
            | SessionOwnerReply::PromptFailed {
                remote: RemoteEffect::None,
                ..
            } => InputAcceptance::Unsent { reason: None },
            _ => InputAcceptance::Unknown,
        };
        self.accepted(operation, acceptance);
        if let Some(caller) = lock(&self.replies).remove(&operation) {
            let _caller_gone = caller.send(OwnedSettlement {
                reply,
                saved,
                idle_steer_summary,
                turn_approver: joined.and_then(|target| target.different_approver),
            });
            self.reply_changed.notify_waiters();
        }
    }
}

fn lock<T>(value: &Mutex<T>) -> MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
