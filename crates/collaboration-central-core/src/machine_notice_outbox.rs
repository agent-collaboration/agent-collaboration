//! Each sender's journal owns its notice debt; alarms drive the same Delivery call.
use crate::machine_notice_delivery::DeliverMachineNotice;
use board_sync_protocol::BoardNotice;
use durable_steps::{AtomicStore, OperationContext, OperationFailure, StepName, Task};
use message_board::ServiceId;
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MachineNoticeInput {
    pub installation: ServiceId,
    pub notice: BoardNotice,
}

pub(crate) struct FleetNoticeTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for FleetNoticeTask<Store> {
    // Preserve the existing Fleet workflow's persisted identity and input shape.
    const KIND: &'static str = "central.fleet-notice";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = MachineNoticeInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &MachineNoticeInput,
    ) -> Result<(), OperationFailure> {
        deliver(context, input).await
    }
}

pub(crate) struct RegistryNoticeTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for RegistryNoticeTask<Store> {
    const KIND: &'static str = "central.registry-notice";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = MachineNoticeInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &MachineNoticeInput,
    ) -> Result<(), OperationFailure> {
        deliver(context, input).await
    }
}

pub(crate) struct MailboxNoticeTask<Store>(pub PhantomData<fn() -> Store>);
impl<Store: AtomicStore> Task for MailboxNoticeTask<Store> {
    const KIND: &'static str = "central.mailbox-notice";
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = MachineNoticeInput;
    type Output = ();
    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &MachineNoticeInput,
    ) -> Result<(), OperationFailure> {
        deliver(context, input).await
    }
}

async fn deliver<Context: OperationContext>(
    context: &mut Context,
    input: &MachineNoticeInput,
) -> Result<(), OperationFailure> {
    let step =
        StepName::new("deliver_notice").map_err(|error| OperationFailure::InvalidPayload {
            message: error.to_string(),
        })?;
    context
        .call(
            step,
            DeliverMachineNotice {
                installation: input.installation.clone(),
            },
            input.notice.clone(),
        )
        .await?;
    Ok(())
}
