//! Bind shared copy targets to one configured receiver and one physical database.

use crate::{
    CallerKind, CopyKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo,
    caller_context::CallAuthority,
    copy_mutations::{CopyMutation, CopyMutationAnswer},
    copy_read_calls::CopySessionRead,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, storage::TransactionScope,
};
use fleet_sync_lease::{CopyAcknowledged, CopyCreate, CopyFence, CopyRead, CopyRetire};
use fleet_sync_protocol::{
    AwaitAcked, CopyStatusAnswer, CopyUnavailableReason, CreateCopy, FenceRequest, FenceResult,
    RetireCopy,
};
use serde::{Serialize, de::DeserializeOwned};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> f64:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Vec<u8>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let ObjectWork::Copy {
            database, receiver, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let call = self.admit_copy_call(&bytes).await?;
        if !call.has_fleet_source() {
            return Err(refused());
        }
        if matches!(self.copy_call_class(&bytes)?, crate::CopyCallClass::Health) {
            return Err(refused());
        }
        if call.kind == crate::owner_operation_calls::CopyOperations::KIND {
            return self.owner_copy_call(call).await;
        }
        if !matches!(
            self.inspection()
                .await
                .map_err(|_| CallFailure::AnswerLost)?,
            fleet_sync_receiver::CopyInspection::Retired { .. }
        ) {
            self.initialize().await.map_err(|error| {
                match crate::migration_failure::MigrationFailure::from_rollback(&error) {
                    Some(failure) => fleet_sync_receiver::retryable_copy(failure.unavailable()),
                    None => fleet_sync_receiver::classify_copy_store_error(error).call_failure(),
                }
            })?;
        }
        if call.kind == crate::copy_write_calls::CopyAdminSeed::KIND {
            return self.admin_seed_call(call).await;
        }
        if call.kind == crate::copy_write_calls::CopySessionWrite::KIND {
            return self.writer_copy_call(call).await;
        }
        if call.kind == crate::admin_import_copy::AdminCopySeed::KIND {
            return self.admin_copy_seed(call).await;
        }
        if !matches!(
            call.authority,
            CallAuthority::Object | CallAuthority::Machine { .. }
        ) {
            return Err(refused());
        }
        if call.kind == CopySessionRead::KIND {
            let target: CopySessionRead = decode(call.target.clone())?;
            if call.source_face != CallerKind::Session
                || !call.has_fleet_source()
                || target.copy != *receiver
                || target.database != *database
            {
                return Err(refused());
            }
            return encode(self.read_copy(decode(call.request)?).await);
        }
        if call.kind == CopyRead::KIND {
            let target: CopyRead = decode(call.target)?;
            let _: () = decode(call.request)?;
            if target.copy != *receiver || target.database != *database {
                return Err(refused());
            }
            let answer = self
                .read_copy_status()
                .await
                .map_err(|failure| failure.call_failure())?;
            if answer == CopyStatusAnswer::NotInitialized {
                return Err(uninitialized());
            }
            return encode(answer);
        }
        if call.source_face != CallerKind::Registry || !call.has_fleet_source() {
            return Err(refused());
        }
        #[cfg(feature = "conformance")]
        if call.kind == crate::conformance_copy_wait::CopyWaitTarget::KIND {
            let target: crate::conformance_copy_wait::CopyWaitTarget = decode(call.target)?;
            if target.copy != *receiver || target.database != *database {
                return Err(refused());
            }
            let observed = self
                .copy_wait_control(decode(call.request)?)
                .map_err(|_| CallFailure::AnswerLost)?;
            return encode(observed);
        }
        let mutation = match call.kind.as_str() {
            CopyAcknowledged::KIND => {
                let target: CopyAcknowledged = decode(call.target)?;
                let request: AwaitAcked = decode(call.request)?;
                if target.copy != *receiver || target.database != *database {
                    return Err(refused());
                }
                #[cfg(feature = "conformance")]
                self.record_copy_wait_call(call.identity)?;
                return encode(self.await_copy_acked(request.through).await?);
            }
            CopyCreate::KIND => {
                let target: CopyCreate = decode(call.target)?;
                let request: CreateCopy = decode(call.request)?;
                if target.copy != *receiver || request.database != *database {
                    return Err(refused());
                }
                CopyMutation::Create(request)
            }
            CopyFence::KIND => {
                let target: CopyFence = decode(call.target)?;
                let request: FenceRequest = decode(call.request)?;
                if target.copy != *receiver || request.database != *database {
                    return Err(refused());
                }
                CopyMutation::Fence(request)
            }
            CopyRetire::KIND => {
                let target: CopyRetire = decode(call.target)?;
                let request: RetireCopy = decode(call.request)?;
                if target.copy != *receiver || request.database != *database {
                    return Err(refused());
                }
                CopyMutation::Retire(request)
            }
            _ => return Err(refused()),
        };
        match self.mutate_copy(mutation).await? {
            CopyMutationAnswer::Created(answer) => encode(answer),
            CopyMutationAnswer::Fenced(FenceResult::NotInitialized) => Err(uninitialized()),
            CopyMutationAnswer::Fenced(answer) => encode(answer),
            CopyMutationAnswer::Retired(answer) => encode(answer),
        }
    }
}

fn decode<T: DeserializeOwned>(value: serde_json::Value) -> Result<T, CallFailure> {
    serde_json::from_value(value).map_err(encode_refusal)
}
fn encode(answer: impl Serialize) -> Result<Vec<u8>, CallFailure> {
    serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
}
fn uninitialized() -> CallFailure {
    fleet_sync_receiver::retryable_copy(CopyUnavailableReason::Uninitialized)
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<crate::FleetKind, Host, Io>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(crate) async fn owner_project_call(
        &self,
        mut call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        use crate::{
            admin_import_copy::{
                AdminCopySeed, AdminSeedAnswer, AdminSeedRequest, FleetAdminCopySeed,
            },
            copy_write_calls::{CopyAdminSeed, FleetProjectSeed},
            owner_operation_calls::{CopyOperations, FleetProjectOperations},
        };
        if !call.has_fleet_source()
            || call.source_face != CallerKind::Edge
            || !matches!(call.authority, CallAuthority::Owner(_))
        {
            return Err(refused());
        }
        let ObjectWork::Fleet { io, host, .. } = &self.work else {
            return Err(refused());
        };
        let (database, kind, target, publication) = match call.kind.as_str() {
            FleetProjectOperations::KIND => {
                let target: FleetProjectOperations = decode(call.target)?;
                let request: crate::owner_operation_calls::CopyOpsCall =
                    decode(call.request.clone())?;
                call.request = serde_json::to_value(request).map_err(encode_refusal)?;
                (
                    target.database,
                    CopyOperations::KIND,
                    serde_json::to_value(CopyOperations {
                        database: target.database,
                    })
                    .map_err(encode_refusal)?,
                    None,
                )
            }
            FleetAdminCopySeed::KIND => {
                let target: FleetAdminCopySeed = decode(call.target)?;
                let request: AdminSeedRequest = decode(call.request.clone())?;
                let publication = match request {
                    AdminSeedRequest::Complete(record)
                    | AdminSeedRequest::ReconcilePublication(record) => {
                        if record.header.database() != target.database {
                            return Err(refused());
                        }
                        Some(record)
                    }
                    AdminSeedRequest::Begin { .. } | AdminSeedRequest::Page { .. } => None,
                };
                (
                    target.database,
                    AdminCopySeed::KIND,
                    serde_json::to_value(AdminCopySeed {
                        database: target.database,
                    })
                    .map_err(encode_refusal)?,
                    publication,
                )
            }
            FleetProjectSeed::KIND => {
                let target: FleetProjectSeed = decode(call.target)?;
                let request: crate::copy_write_calls::AdminCopySeedRequest =
                    decode(call.request.clone())?;
                call.request = serde_json::to_value(request).map_err(encode_refusal)?;
                (
                    target.database,
                    CopyAdminSeed::KIND,
                    serde_json::to_value(CopyAdminSeed {
                        database: target.database,
                        copy: target.copy,
                    })
                    .map_err(encode_refusal)?,
                    None,
                )
            }
            _ => return Err(refused()),
        };
        call.source_face = CallerKind::Fleet;
        call.destination = ObjectAddress::Copy(database);
        call.destination_face = CallerKind::Copy;
        call.kind = kind.into();
        call.target = target;
        // No Fleet transaction is held while the project executes.
        let bytes = serde_json::to_vec(&call).map_err(encode_refusal)?;
        let answer = io.object_call(call.destination, bytes).await?;
        if let Some(record) = publication
            && let AdminSeedAnswer::Published { position } =
                serde_json::from_slice(&answer).map_err(|_| CallFailure::AnswerLost)?
        {
            if position != record.header.manifest.position {
                return Err(CallFailure::AnswerLost);
            }
            // Fleet owns import bookkeeping; the project returns its committed publication evidence.
            let (recorded, _) = crate::step_driver::StepDriver::new(host.clone())
                .run(crate::admin_import_store::AdminImportStep {
                    now: io.now(),
                    row_ids: crate::fleet_row_ids::FleetRowIds::new(io.clone()),
                    command: crate::admin_import_store::AdminImportCommand::Published {
                        database,
                        digest: record.header.digest.clone(),
                        import_id: record.import_id,
                        position,
                    },
                })
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
            recorded.map_err(|_| CallFailure::AnswerLost)?;
        }
        Ok(answer)
    }
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<CopyKind, Host, Io>
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
    pub(crate) async fn admit_copy_call(
        &self,
        bytes: &[u8],
    ) -> Result<ObjectCallEnvelope, CallFailure> {
        let ObjectWork::Copy {
            database,
            receiver,
            fleet_references,
            ..
        } = &self.work
        else {
            return Err(refused());
        };
        let call = ObjectCallEnvelope::decode_copy_candidate(bytes, *database)?;
        match fleet_references.resolve(*database).await {
            Ok(organization) if organization == call.organization => return Ok(call),
            Ok(_) => return Err(refused()),
            Err(crate::CopyFleetReferenceRefusal::Missing) => {}
            Err(_) => return Err(refused()),
        }
        // Only a validated creation/seed admission can establish the private
        // reference. Reads, health, writers and repairs never guess a Fleet.
        let admission = if call.kind == CopyCreate::KIND
            && call.source_face == CallerKind::Registry
            && matches!(call.authority, CallAuthority::Object)
        {
            let target: CopyCreate = decode(call.target.clone())?;
            let request: CreateCopy = decode(call.request.clone())?;
            target.copy == *receiver && request.database == *database
        } else if call.source_face == CallerKind::Fleet
            && matches!(call.authority, CallAuthority::Owner(_))
            && call.kind == crate::copy_write_calls::CopyAdminSeed::KIND
        {
            let target: crate::copy_write_calls::CopyAdminSeed = decode(call.target.clone())?;
            let request: crate::copy_write_calls::AdminCopySeedRequest =
                decode(call.request.clone())?;
            target.copy == *receiver
                && target.database == *database
                && matches!(request, crate::copy_write_calls::AdminCopySeedRequest::Begin { database: requested, .. } if requested == *database)
        } else if call.source_face == CallerKind::Fleet
            && matches!(call.authority, CallAuthority::Owner(_))
            && call.kind == crate::admin_import_copy::AdminCopySeed::KIND
        {
            let target: crate::admin_import_copy::AdminCopySeed = decode(call.target.clone())?;
            let request: crate::admin_import_copy::AdminSeedRequest = decode(call.request.clone())?;
            target.database == *database
                && matches!(
                    request,
                    crate::admin_import_copy::AdminSeedRequest::Begin { .. }
                )
        } else {
            false
        };
        if !admission
            || !matches!(
                self.inspection().await.map_err(|_| refused())?,
                fleet_sync_receiver::CopyInspection::Uninitialized
            )
        {
            return Err(refused());
        }
        fleet_references
            .bind(*database, call.organization.clone())
            .await
            .map_err(|_| refused())?;
        Ok(call)
    }
}
