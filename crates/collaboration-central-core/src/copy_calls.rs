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
        let address = ObjectAddress::Copy(*database);
        let call = ObjectCallEnvelope::decode(&bytes, address, CallerKind::Copy)?;
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
        if !matches!(call.authority, CallAuthority::Object) {
            return Err(refused());
        }
        if call.kind == CopySessionRead::KIND {
            let target: CopySessionRead = decode(call.target)?;
            if call.source_face != CallerKind::Session
                || !matches!(call.source, ObjectAddress::Connection(_))
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
        if call.source_face != CallerKind::Registry || call.source != ObjectAddress::Registry {
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
