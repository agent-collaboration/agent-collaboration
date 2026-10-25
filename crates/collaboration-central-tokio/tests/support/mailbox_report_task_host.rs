//! A real SDK report call crosses the authenticated conformance socket.
use super::super::{mailbox_rule_fixture::MailboxPair, relay_transport::operation};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{CallFailure, CallId, CallTarget, Host, Uuid};
use fleet_sync_delivery::ReportCall;
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryFailureKind, DeliveryRefusal, OperationAnswer, ReportResult,
};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

pub(super) type SharedPair = Arc<Mutex<MailboxPair>>;
pub(super) struct ReportHost {
    pair: SharedPair,
    now: AtomicI64,
    dispatch_attempts: AtomicUsize,
    terminal_refusal: AtomicBool,
}
impl ReportHost {
    pub(super) fn new(pair: SharedPair) -> Self {
        Self {
            pair,
            now: AtomicI64::new(1_893_456_000_000),
            dispatch_attempts: AtomicUsize::new(0),
            terminal_refusal: AtomicBool::new(false),
        }
    }
    pub(super) fn advance(&self, millis: i64) {
        self.now.fetch_add(millis, Ordering::SeqCst);
    }
    pub(super) fn dispatch_attempts(&self) -> usize {
        self.dispatch_attempts.load(Ordering::SeqCst)
    }
    pub(super) fn terminal_refusal(&self) -> bool {
        self.terminal_refusal.load(Ordering::SeqCst)
    }
}
impl Clock for ReportHost {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.now.load(Ordering::SeqCst))
    }
}
impl IdSource for ReportHost {
    fn new_id(&self) -> Uuid {
        Uuid::now_v7()
    }
}
impl WakeScheduler for ReportHost {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl Host for ReportHost {
    async fn dispatch<C: CallTarget>(
        &self,
        target: C,
        report: C::Request,
        call: CallId,
    ) -> Result<C::Answer, CallFailure> {
        if C::KIND != ReportCall::<BoardDeliveryTypes>::KIND {
            return Err(CallFailure::AnswerLost);
        }
        let target: ReportCall<BoardDeliveryTypes> = serde_json::from_value(
            serde_json::to_value(target).map_err(|_| CallFailure::AnswerLost)?,
        )
        .map_err(|_| CallFailure::AnswerLost)?;
        let mut pair = self.pair.lock().await;
        if target.mailbox.as_uuid() != pair.recipient.peer {
            return Err(CallFailure::AnswerLost);
        }
        self.dispatch_attempts.fetch_add(1, Ordering::SeqCst);
        let answer = operation(
            &mut pair.recipient.socket,
            json!({
                "kind":"delivery","value":{"kind":"report","value":{"call":call,"report":report}}
            }),
        )
        .await
        .map_err(|_| CallFailure::AnswerLost)?;
        let answer: OperationAnswer<BoardConsumerProtocol> =
            serde_json::from_value(answer).map_err(|_| CallFailure::AnswerLost)?;
        let OperationAnswer::Delivery(DeliveryAnswer::Report(answer)) = answer else {
            return Err(CallFailure::AnswerLost);
        };
        if matches!(
            &answer,
            ReportResult::Refused {
                reason: DeliveryRefusal::ProcessingFailed {
                    kind: DeliveryFailureKind::ExecutionFailed
                }
            }
        ) {
            self.terminal_refusal.store(true, Ordering::SeqCst);
        }
        serde_json::from_value(serde_json::to_value(answer).map_err(|_| CallFailure::AnswerLost)?)
            .map_err(|_| CallFailure::AnswerLost)
    }
}
