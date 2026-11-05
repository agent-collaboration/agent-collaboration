//! Current admission authority is distinct from retained historical peer identity.
use crate::machine_enrollment_record::{
    EnrollmentIdentity, EnrollmentState, StoredMachine, corrupt,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_protocol::PeerId;
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn read_current_enrolled_machine<S: TransactionScope>(
    scope: &mut S,
    peer: PeerId,
    now: SyncTime,
) -> Result<Option<EnrollmentIdentity>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database> + Decode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database> + Encode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let mut rows = crate::generated_enrollment::read_enrollments(
        scope,
        None,
        None,
        None,
        Some(peer.to_string()),
    )
    .await
    .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("peer_id"));
    }
    let Some(machine) = rows.pop().map(StoredMachine::decode).transpose()? else {
        return Ok(None);
    };
    if machine.peer != peer {
        return Err(corrupt("peer_id"));
    }
    Ok((machine.state == EnrollmentState::Enrolled
        && machine.expires_at.timestamp_millis() > now.unix_millis())
    .then_some(EnrollmentIdentity {
        peer: machine.peer,
        origin: machine.origin,
        last_seen: machine.last_seen,
    }))
}
