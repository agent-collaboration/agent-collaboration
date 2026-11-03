//! Reconciliation authorizes a recorded project against its current Fleet holder.
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_sync_delivery::{DeliveryLimits, Outbox};
use fleet_sync_protocol::{ItemId, PeerId, Submitter};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

pub(crate) async fn authorized_submitter<S: TransactionScope>(
    scope: &mut S,
    peer: PeerId,
    item: ItemId,
    limits: DeliveryLimits,
    now: runtime_primitives::SyncTime,
) -> Result<Option<Submitter>, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let owner = Outbox::<_, BoardDeliveryTypes, _>::new(
        scope,
        limits,
        &crate::fleet_sender_binding::FleetSenderBinding,
    )
    .relay_submitter(item)
    .await
    .map_err(fleet_sync_delivery::delivery_rollback)?;
    match owner {
        Some(Submitter::Project(database)) => {
            let lease = fleet_sync_lease::lease_status(scope, database, now)
                .await
                .map_err(StoreError::backend)?;
            Ok(lease
                .filter(|lease| {
                    lease.state == fleet_sync_protocol::LeaseState::Held && lease.holder == peer
                })
                .map(|_| Submitter::Project(database)))
        }
        // The SDK compares a known machine owner to this independently
        // authenticated peer; unknown settlements bind the same machine claim.
        None | Some(Submitter::Machine(_)) => Ok(Some(Submitter::Machine(peer))),
    }
}
