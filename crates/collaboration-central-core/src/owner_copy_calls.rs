//! Repair is the existing Copy alarm path, with retirement checked first.
use crate::{
    CallerKind, CopyKind, ObjectPlatform, ObjectRuntime,
    caller_context::CallAuthority,
    object_call_envelope::ObjectCallEnvelope,
    object_runtime::ObjectWork,
    owner_operation_calls::{CopyOperations, CopyOpsAnswer, CopyOpsCall},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, StoreError, storage::TransactionScope,
};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
/// The shell can measure Health before initialization; core owns classification.
pub enum CopyCallClass {
    Health,
    Ordinary,
}
impl<Host: ObjectPlatform> ObjectRuntime<CopyKind, Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Decode<'value, Host::Database> + Encode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn copy_call_class(&self, bytes: &[u8]) -> Result<CopyCallClass, CallFailure> {
        let ObjectWork::Copy { database, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let call = ObjectCallEnvelope::decode(
            bytes,
            crate::ObjectAddress::Copy(*database),
            CallerKind::Copy,
        )?;
        if call.kind != CopyOperations::KIND {
            return Ok(CopyCallClass::Ordinary);
        }
        let request = self.decode_owner_operation(call)?;
        Ok(match request {
            CopyOpsCall::Health => CopyCallClass::Health,
            CopyOpsCall::Repair | CopyOpsCall::Migrate => CopyCallClass::Ordinary,
        })
    }
    fn decode_owner_operation(&self, call: ObjectCallEnvelope) -> Result<CopyOpsCall, CallFailure> {
        if call.source_face != CallerKind::Fleet
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let target: CopyOperations = serde_json::from_value(call.target)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let ObjectWork::Copy { database, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        if *database != target.database {
            return Err(CallFailure::AnswerLost);
        }
        serde_json::from_value(call.request).map_err(crate::object_call_envelope::encode_refusal)
    }
    pub async fn on_health(
        &self,
        bytes: Vec<u8>,
        storage_bytes: u64,
    ) -> Result<Vec<u8>, CallFailure> {
        if !matches!(self.copy_call_class(&bytes)?, CopyCallClass::Health) {
            return Err(CallFailure::AnswerLost);
        }
        let ObjectWork::Copy { host, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        let (health, _) = crate::step_driver::StepDriver::new(host.clone())
            .run(crate::copy_health::ReadCopyHealth { storage_bytes })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        serde_json::to_vec(&CopyOpsAnswer::Health(Box::new(health)))
            .map_err(|_| CallFailure::AnswerLost)
    }
    pub(crate) async fn inspection(
        &self,
    ) -> Result<fleet_project_copy::CopyInspection, StoreError> {
        let ObjectWork::Copy { host, .. } = &self.work else {
            return Err(StoreError::backend("copy inspection reached another face"));
        };
        let (inspection, _) = crate::step_driver::StepDriver::new(host.clone())
            .run(ReadInspection)
            .await?;
        Ok(inspection)
    }
    pub(crate) async fn owner_copy_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if call.source_face != CallerKind::Fleet
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(CallFailure::AnswerLost);
        }
        let target: CopyOperations = serde_json::from_value(call.target)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let request: CopyOpsCall = serde_json::from_value(call.request)
            .map_err(crate::object_call_envelope::encode_refusal)?;
        let ObjectWork::Copy { database, .. } = &self.work else {
            return Err(CallFailure::AnswerLost);
        };
        if *database != target.database {
            return Err(CallFailure::AnswerLost);
        }
        let answer = match request {
            // The transport must supply the target measurement through on_health.
            CopyOpsCall::Health => return Err(CallFailure::AnswerLost),
            CopyOpsCall::Migrate => {
                let ObjectWork::Copy { host, io, .. } = &self.work else {
                    return Err(CallFailure::AnswerLost);
                };
                CopyOpsAnswer::Migration(
                    crate::ObjectBootstrap::new(host.clone(), io.clone())
                        .migrate_copy()
                        .await,
                )
            }
            CopyOpsCall::Repair => {
                // Owner operations are dispatched before the ordinary prelude.
                // Repair still admits migrations, except for a retired receipt.
                if !matches!(
                    self.inspection()
                        .await
                        .map_err(|_| CallFailure::AnswerLost)?,
                    fleet_project_copy::CopyInspection::Retired { .. }
                ) {
                    self.initialize().await.map_err(|error| {
                        match crate::migration_failure::MigrationFailure::from_rollback(&error) {
                            Some(failure) => {
                                fleet_project_copy::retryable_copy(failure.unavailable())
                            }
                            None => {
                                fleet_project_copy::classify_copy_store_error(error).call_failure()
                            }
                        }
                    })?;
                }
                match self
                    .read_copy_status()
                    .await
                    .map_err(fleet_project_copy::CopyTransactionFailure::call_failure)?
                {
                    fleet_protocol::CopyStatusAnswer::Retired => CopyOpsAnswer::Retired,
                    fleet_protocol::CopyStatusAnswer::Status(_)
                    | fleet_protocol::CopyStatusAnswer::NotInitialized => {
                        self.expire_copy_staged_parts()
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?;
                        self.drive_copy_followups()
                            .await
                            .map_err(|_| CallFailure::AnswerLost)?;
                        CopyOpsAnswer::Repaired
                    }
                }
            }
        };
        serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}
struct ReadInspection;
impl<Store: AtomicStore, Driver: Database> crate::step_driver::StepCallback<Store, Driver>
    for ReadInspection
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Driver>
        + sqlx_core::encode::Encode<'blob, Driver>
        + sqlx_core::decode::Decode<'blob, Driver>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Driver> + sqlx_core::decode::Decode<'blob, Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Decode<'value, Driver> + Encode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = fleet_project_copy::CopyInspection;
    type Effect = std::convert::Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, crate::step_driver::StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move { Ok((fleet_project_copy::inspect_copy(scope).await, Vec::new())) })
    }
}
