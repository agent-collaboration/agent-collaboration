//! Every mailbox request shares one transaction and post-rollback health boundary.
use crate::{
    MailboxKind, ObjectHost, ObjectRuntime, PlatformIo,
    object_runtime::ObjectWork,
    step_driver::{StepCallback, StepDriver},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{AtomicStore, CallFailure, StoreError, storage::TransactionScope};
use fleet_sync_delivery::{
    MailboxCorruption, MailboxFailureDisposition, MailboxOperation, classify_delivery_store_error,
    mailbox_failure_disposition,
};
use fleet_sync_protocol::{DeliveryAnswer, DeliveryFailureKind, DeliveryRefusal, PeerId};
use runtime_primitives::{Clock, WakeScheduler};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::sync::Mutex;

#[derive(Default)]
pub(crate) struct MailboxHealth(Mutex<Option<DeliveryFailureKind>>);
impl MailboxHealth {
    pub(crate) fn failure(&self) -> Result<Option<DeliveryFailureKind>, StoreError> {
        self.0
            .lock()
            .map(|failure| *failure)
            .map_err(StoreError::backend)
    }
    fn mark_corrupt(&self, corruption: MailboxCorruption) -> Result<(), CallFailure> {
        let mut failure = self.0.lock().map_err(|_| CallFailure::AnswerLost)?;
        if failure.is_none() {
            *failure = Some(DeliveryFailureKind::CustodyCorrupt);
            eprintln!(
                "mailbox {} unavailable: invalid custody field {}",
                corruption.mailbox, corruption.field
            );
        }
        Ok(())
    }
}
pub(crate) struct ActivateMailboxWork;

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<MailboxKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn run_mailbox_step<Callback>(
        &self,
        recipient: PeerId,
        operation: MailboxOperation,
        callback: Callback,
    ) -> Result<Vec<u8>, CallFailure>
    where
        Callback: StepCallback<
                Host::Store,
                Host::Database,
                Output = DeliveryAnswer<BoardDeliveryTypes>,
                Effect = ActivateMailboxWork,
            >,
    {
        let ObjectWork::Mailbox {
            host,
            shared_host,
            health,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let exclusion = host.clone();
        let host = host.clone();
        let health = health.clone();
        // Serialize the health read, SQL step and final rollback classification.
        // The mutex protects only the health value; it is never held across await.
        let (answer, effects) = exclusion
            .with_local_section(async move {
                if let Some(kind) = health.failure().map_err(|_| CallFailure::AnswerLost)? {
                    return Ok((
                        DeliveryAnswer::MailboxUnavailable {
                            reason: DeliveryRefusal::ProcessingFailed { kind },
                        },
                        Vec::new(),
                    ));
                }
                match StepDriver::new(host).run(callback).await {
                    Ok((answer, committed)) => Ok((answer, committed.into_effects())),
                    Err(error) => match mailbox_failure_disposition::<BoardDeliveryTypes>(
                        recipient,
                        operation,
                        classify_delivery_store_error(error),
                    ) {
                        MailboxFailureDisposition::Answer { answer, corruption } => {
                            if let Some(corruption) = corruption {
                                health.mark_corrupt(corruption)?;
                            }
                            Ok((answer, Vec::new()))
                        }
                        MailboxFailureDisposition::Unhandled(_) => Err(CallFailure::AnswerLost),
                    },
                }
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)??;
        for ActivateMailboxWork in effects {
            // Failed activation loses the answer, preserving the committed rule
            // result and its original item/call identity for an exact retry.
            exclusion
                .wake()
                .wake_at(shared_host.now())
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
