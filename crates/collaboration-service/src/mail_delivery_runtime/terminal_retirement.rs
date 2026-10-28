//! Direct mail and wake retirement keep their route and call identity in the existing journal.
use super::{
    CallEnvelope, CallFailure, CallTarget, CarrierAcceptance, DeliveryLimits, ItemId,
    MailCallRouter, MailHost, MailStore, PeerId, RouterDeliveryTypes, StoreError, TursoScope,
    Value,
};
use durable_steps::{JournalScope, OperationContext, OperationFailure, StepError, Task};
use fleet_sync_delivery::{
    Outbox, OutboxAdmission, OutboxStatusState, SubmitInput, delivery_rollback,
};
use fleet_sync_protocol::{
    DeliveryAnswer, DeliveryRequest, OperationAnswer, OperationRequest, RetireResult,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(super) struct RetireTerminalMail {
    pub limits: DeliveryLimits,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct RetireMailCall {
    relay: PeerId,
    mailbox: PeerId,
}
impl CallTarget for RetireMailCall {
    const KIND: &'static str = "board.mail.retire-terminal";
    type Request = ItemId;
    type Answer = RetireResult;
}

pub(super) async fn admit_terminal_retirement(
    scope: &mut TursoScope<'_, MailHost>,
    admission: &OutboxAdmission<CarrierAcceptance>,
    input: SubmitInput,
) -> Result<(), StoreError> {
    // Replay already has its original retirement obligation. In particular,
    // never compare that frozen route with a newly configured session relay.
    if matches!(admission, OutboxAdmission::Queued { .. }) {
        scope
            .admit::<RetireTerminalMail>(
                &fleet_sync_delivery::delivery_request_key(&input.item)
                    .map_err(delivery_rollback)?,
                &input,
            )
            .await
            .map_err(StoreError::backend)?;
    }
    Ok(())
}

impl Task for RetireTerminalMail {
    const KIND: &'static str = "board.mail.retire-terminal";
    const VERSION: u32 = 1;
    const KEEP_RESULT: bool = false;
    type Store = MailStore;
    type Input = SubmitInput;
    type Output = ();

    async fn run<O: OperationContext<Store = MailStore>>(
        &self,
        ctx: &mut O,
        input: &SubmitInput,
    ) -> Result<(), OperationFailure> {
        let item = input.item;
        let limits = self.limits;
        let canonical = ctx
            .local(step("terminal-status")?, move |scope| {
                Box::pin(async move {
                    let state = Outbox::<_, RouterDeliveryTypes>::new(scope, limits)
                        .status(item)
                        .await
                        .map_err(delivery_rollback)?
                        .state;
                    match state {
                        OutboxStatusState::Known {
                            status, version, ..
                        } if status.is_terminal() => {
                            // A local withdrawal or rejected admission never entered central custody.
                            Ok(version.value() != 0)
                        }
                        OutboxStatusState::Unknown | OutboxStatusState::Known { .. } => {
                            Err(StepError::Retry {
                                after: Duration::from_secs(5),
                            })
                        }
                    }
                })
            })
            .await?;
        if !canonical {
            return Ok(());
        }
        let answer = ctx
            .call(
                step("retire")?,
                RetireMailCall {
                    relay: input.relay,
                    mailbox: input.mailbox,
                },
                item,
            )
            .await?;
        if answer.retired != [item]
            || !answer.unknown.is_empty()
            || !answer.not_current_target.is_empty()
        {
            return Err(OperationFailure::InvalidPayload {
                message: "terminal retirement did not confirm its exact item".into(),
            });
        }
        Ok(())
    }
}

fn step(name: &str) -> Result<runtime_primitives::StepName, OperationFailure> {
    runtime_primitives::StepName::new(name).map_err(|error| OperationFailure::InvalidPayload {
        message: error.to_string(),
    })
}

impl MailCallRouter {
    pub(super) async fn dispatch_retirement(
        &self,
        envelope: CallEnvelope,
    ) -> Result<Value, CallFailure> {
        let target: RetireMailCall =
            serde_json::from_value(envelope.target).map_err(|_| CallFailure::AnswerLost)?;
        let item: ItemId =
            serde_json::from_value(envelope.request).map_err(|_| CallFailure::AnswerLost)?;
        let session = self.session.as_ref().ok_or(CallFailure::AnswerLost)?;
        if target.relay != session.relay {
            return Err(CallFailure::AnswerLost);
        }
        match session
            .requests
            .request(OperationRequest::Delivery(DeliveryRequest::Retire {
                call: envelope.call,
                mailbox: target.mailbox,
                items: vec![item],
            }))
            .await?
        {
            OperationAnswer::Delivery(DeliveryAnswer::Retire(answer)) => {
                serde_json::to_value(answer).map_err(|_| CallFailure::AnswerLost)
            }
            _ => Err(CallFailure::AnswerLost),
        }
    }
}
