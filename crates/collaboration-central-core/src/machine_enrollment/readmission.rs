//! Readmission opens the mailbox before its reserved identities become active.

use super::*;
use durable_steps::{OperationContext, OperationFailure, Task};
use std::marker::PhantomData;
#[cfg(feature = "conformance")]
use std::sync::Arc;

pub(super) const READMISSION_TASK_KIND: &str = "central.readmit-machine";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadmissionInput {
    pub peer: PeerId,
}
pub(crate) struct ReadmissionTask<Store, Driver> {
    pub marker: PhantomData<fn() -> (Store, Driver)>,
    #[cfg(feature = "conformance")]
    pub probe: Arc<super::conformance_revocation::RevocationProbe>,
}
impl<Store: AtomicStore, Driver: Database> Task for ReadmissionTask<Store, Driver>
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    const KIND: &'static str = READMISSION_TASK_KIND;
    const VERSION: u32 = 1;
    type Store = Store;
    type Input = ReadmissionInput;
    type Output = ();

    async fn run<Context: OperationContext<Store = Store>>(
        &self,
        context: &mut Context,
        input: &ReadmissionInput,
    ) -> Result<(), OperationFailure> {
        context
            .call(
                durable_steps::StepName::new("open_mailbox").map_err(invalid_step)?,
                crate::mailbox_lifecycle::OpenPeerMailbox { peer: input.peer },
                (),
            )
            .await?;
        #[cfg(feature = "conformance")]
        if self
            .probe
            .before_activation_pause
            .defer_completion()
            .map_err(|error| OperationFailure::Storage { error })?
        {
            // The Open answer is saved, but no activation step has run. Yield
            // the alarm so normal credential requests can observe this state.
            return Err(OperationFailure::Retry { after_ms: 100 });
        }
        let peer = input.peer;
        context
            .local(
                durable_steps::StepName::new("activate_enrollment").map_err(invalid_step)?,
                move |scope| {
                    Box::pin(async move {
                        queries::activate_enrollment(scope, peer.to_string())
                            .await
                            .map_err(StoreError::backend)?;
                        Ok(())
                    })
                },
            )
            .await?;
        #[cfg(feature = "conformance")]
        if self
            .probe
            .activation_pause
            .defer_completion()
            .map_err(|error| OperationFailure::Storage { error })?
        {
            // A live held alarm would stop unrelated due operations in the
            // native host. Retry preserves this real local receipt and yields
            // the drive so the test's later revoke can finish first.
            return Err(OperationFailure::Retry { after_ms: 100 });
        }
        Ok(())
    }
}
fn invalid_step(error: impl std::fmt::Display) -> OperationFailure {
    OperationFailure::InvalidPayload {
        message: error.to_string(),
    }
}
