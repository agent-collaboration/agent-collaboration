//! Owner requests admit local work, then drive the existing library outside it.
use crate::{
    CallerKind, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    owner_registry_calls::*,
    owner_registry_steps::*,
    step_driver::StepDriver,
};
use durable_steps::{
    AtomicStore, CallFailure, OperationState, RequestKey, storage::TransactionScope,
};
use fleet_sync_lease::PreviewTakeoverTask;
use fleet_sync_protocol::PreviewTakeover;
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
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
    pub(crate) async fn owner_registry_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Edge
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let OwnerRegistryOperations { database } =
            serde_json::from_value(call.target).map_err(encode_refusal)?;
        let request: OwnerRegistryCall =
            serde_json::from_value(call.request).map_err(encode_refusal)?;
        let ObjectWork::Registry {
            engine,
            host,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let answer = match request {
            OwnerRegistryCall::Preview => {
                let (check, _) = StepDriver::new(host.clone())
                    .run(CheckOwnerPreview {
                        database,
                        now: shared_host.now(),
                    })
                    .await
                    .map_err(|_| CallFailure::AnswerLost)?;
                match check {
                    OwnerPreviewCheck::Refused(answer) => OwnerRegistryAnswer::Preview(answer),
                    OwnerPreviewCheck::Held => {
                        let key = RequestKey::new(format!(
                            "owner-preview:{}",
                            call.identity.operation.as_uuid()
                        ))
                        .map_err(encode_refusal)?;
                        let id = engine
                            .start::<PreviewTakeoverTask<Host::Store, Host::Database>>(
                                &key,
                                PreviewTakeover { database },
                            )
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?;
                        engine
                            .drive_due()
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?;
                        let OperationState::Completed { result } = engine
                            .status(&id)
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?
                            .state
                        else {
                            // The library retains only text for a failed preview.
                            // Preserve the approved ordinary503 precision limit.
                            return Err(CallFailure::AnswerLost);
                        };
                        let preview =
                            serde_json::from_slice(&result).map_err(|_| CallFailure::AnswerLost)?;
                        let (display, _) = StepDriver::new(host.clone())
                            .run(ReadOwnerPreviewDisplay { preview })
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?;
                        OwnerRegistryAnswer::Preview(OwnerPreviewAnswer::Ready(display))
                    }
                }
            }
            OwnerRegistryCall::Move(request) => OwnerRegistryAnswer::Transfer(
                self.owner_transfer(database, OwnerTransferOffer::Move(request))
                    .await?,
            ),
            OwnerRegistryCall::Takeover(request) => OwnerRegistryAnswer::Transfer(
                self.owner_transfer(database, OwnerTransferOffer::Takeover(request))
                    .await?,
            ),
            OwnerRegistryCall::Retire(request) => {
                OwnerRegistryAnswer::Retirement(self.owner_retire(database, request).await?)
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
    async fn owner_transfer(
        &self,
        database: fleet_sync_protocol::DatabaseId,
        offer: OwnerTransferOffer,
    ) -> Result<OwnerTransferAnswer, CallFailure> {
        let ObjectWork::Registry {
            engine,
            host,
            shared_host,
            ..
        } = &self.work
        else {
            return Err(CallFailure::AnswerLost);
        };
        let (prepared, _) = StepDriver::new(host.clone())
            .run(PrepareOwnerTransfer {
                database,
                offer,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let request = match prepared {
            PreparedOwnerTransfer::Admitted(request) => request,
            PreparedOwnerTransfer::Refused(answer) => return Ok(answer),
        };
        engine
            .drive_due()
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        let (answer, _) = StepDriver::new(host.clone())
            .run(ReadOwnerTransfer { request })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        if matches!(
            &answer,
            OwnerTransferAnswer::Moved { .. } | OwnerTransferAnswer::Granted { .. }
        ) {
            let _ = shared_host.request_lease_publication_repair().await;
        }
        Ok(answer)
    }
}
