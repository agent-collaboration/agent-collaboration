//! Query derives recorded ownership; Settle authorizes the caller's independent claim.
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
        Some(submitter) => Ok(claim_is_authorized(scope, peer, submitter, now)
            .await?
            .then_some(submitter)),
        // Query is source-free for an unknown item; no negative custody is
        // created and this does not guess an unknown settlement's owner.
        None => Ok(Some(Submitter::Machine(peer))),
    }
}

pub(crate) async fn claim_is_authorized<S: TransactionScope>(
    scope: &mut S,
    peer: PeerId,
    claim: Submitter,
    now: runtime_primitives::SyncTime,
) -> Result<bool, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    match claim {
        Submitter::Project(database) => {
            let lease = match fleet_sync_lease::lease_status(scope, database, now).await {
                Ok(lease) => lease,
                Err(fleet_sync_lease::LeaseError::Retired) => None,
                Err(error) => return Err(StoreError::backend(error)),
            };
            Ok(lease.is_some_and(|lease| {
                lease.state == fleet_sync_protocol::LeaseState::Held && lease.holder == peer
            }))
        }
        Submitter::Machine(owner) => Ok(owner == peer),
        // D2 owns Fleet-originated sends; a session claim never grants Fleet authority.
        Submitter::Fleet(_) => Ok(false),
    }
}
