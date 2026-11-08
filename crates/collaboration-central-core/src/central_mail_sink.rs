//! Central's owed-work sink for mail: owed rows instead of journal tasks.
//!
//! fleet-mail-delivery calls this in the same transaction as its custody write
//! (see its `OwedPush`/`OwedStatus`/`OwedRemoval`). A push or status
//! publication becomes a waiting `central_work_attempts` row, and the machine
//! it goes to is named to the running event, whose post-event step sends it
//! within that machine's window. A record's removal is owed to nobody here:
//! the mail retention due source removes records whose retention ended.

use crate::work_attempts::{AttemptKind, AttemptState, socket_requests};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use fleet_mail_delivery::{
    DeliveryError, OwedPush, OwedRemoval, OwedStatus, PublicationRef, PublicationTarget, PushInput,
};
use fleet_protocol::{DeliveryTypes, ItemId, PeerId, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::future::Future;

/// Central's sink for the work a mail custody write owes.
pub(crate) struct CentralMailSink;

type StatusRoute = <BoardDeliveryTypes as DeliveryTypes>::StatusRoute;

fn storage(error: StoreError) -> DeliveryError {
    DeliveryError::Storage(error.to_string())
}

impl<S> OwedPush<S> for CentralMailSink
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String: Type<S::Database> + Encode<'value, S::Database>,
    for<'value> Option<String>: Type<S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "the sink trait requires a Send future borrowing the transaction scope"
    )]
    fn owe_push<'call>(
        scope: &'call mut S,
        input: &'call PushInput,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        async move {
            socket_requests::owe(
                scope,
                input.reference.item.as_uuid(),
                AttemptKind::Push,
                now,
            )
            .await
            .map_err(storage)?;
            crate::event_owed::owed_request(input.recipient);
            Ok(())
        }
    }
}

impl<S> OwedStatus<S, BoardDeliveryTypes> for CentralMailSink
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    #[expect(
        clippy::manual_async_fn,
        reason = "the sink trait requires a Send future borrowing the transaction scope"
    )]
    fn owe_status<'call>(
        scope: &'call mut S,
        reference: &'call PublicationRef<StatusRoute>,
        now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        async move {
            // A Fleet status stream has no socket destination (today's
            // publication refuses it as an invalid route), so nothing is owed.
            if matches!(reference.target, PublicationTarget::StatusStream { .. }) {
                return Ok(());
            }
            let Some(subject) =
                fleet_mail_delivery::publication_subject::<_, BoardDeliveryTypes>(scope, reference)
                    .await?
            else {
                return Ok(());
            };
            let subject = Uuid::parse_str(&subject).map_err(|_| DeliveryError::Corrupt {
                field: "owed publication id",
            })?;
            let existing = crate::work_attempts::attempt(scope, subject, AttemptKind::Status)
                .await
                .map_err(storage)?;
            match existing.map(|row| row.state) {
                None => socket_requests::owe(scope, subject, AttemptKind::Status, now)
                    .await
                    .map_err(storage)?,
                // A newer version replaces what was sent: a late answer to the
                // older send is then stale and can't retire this version.
                Some(AttemptState::InFlight { send, .. })
                    if send
                        .sent_version
                        .is_none_or(|sent| sent < reference.version.value()) =>
                {
                    socket_requests::rewait(scope, subject, AttemptKind::Status, now)
                        .await
                        .map_err(storage)?;
                }
                Some(AttemptState::GivenUp) => {
                    socket_requests::rewait(scope, subject, AttemptKind::Status, now)
                        .await
                        .map_err(storage)?;
                }
                Some(AttemptState::Waiting { .. } | AttemptState::InFlight { .. }) => {}
            }
            if let Some(destination) = destination(scope, &reference.target, now).await? {
                crate::event_owed::owed_request(destination);
            }
            Ok(())
        }
    }
}

impl<S> OwedRemoval<S> for CentralMailSink {
    #[expect(
        clippy::manual_async_fn,
        reason = "the sink trait requires a Send future borrowing the transaction scope"
    )]
    fn owe_record_removal<'call>(
        _scope: &'call mut S,
        _item: ItemId,
        _now: SyncTime,
    ) -> impl Future<Output = Result<(), DeliveryError>> + Send + 'call {
        // The mail retention due source removes records whose retention ended.
        async { Ok(()) }
    }
}

/// The machine a status publication goes to now: the peer itself, or the
/// project's current holder (none while the project is not held).
pub(crate) async fn destination<S>(
    scope: &mut S,
    target: &PublicationTarget<StatusRoute>,
    now: SyncTime,
) -> Result<Option<PeerId>, DeliveryError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    match target {
        PublicationTarget::Peer { peer, .. } => Ok(Some(*peer)),
        PublicationTarget::Project { project, .. } => {
            match fleet_project_lease::lease_status(scope, *project, now).await {
                Ok(lease) => Ok(lease
                    .filter(|lease| lease.state == fleet_protocol::LeaseState::Held)
                    .map(|lease| lease.holder)),
                Err(fleet_project_lease::LeaseError::Retired) => Ok(None),
                Err(error) => Err(DeliveryError::Storage(error.to_string())),
            }
        }
        PublicationTarget::StatusStream { .. } => Ok(None),
    }
}

/// Close the owed pushes whose item's `valid_until` passed before a handoff, at
/// most 100: the item expires if no push (possibly) reached its recipient, or
/// waits for the recipient's report if one may have. Either way the sender is
/// owed its status publication, and the push row is closed.
pub(crate) async fn expire_owed_pushes<S>(scope: &mut S, now: SyncTime) -> Result<(), StoreError>
where
    S: TransactionScope,
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'value> String:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'value> Option<String>:
        Type<S::Database> + Encode<'value, S::Database> + Decode<'value, S::Database>,
    for<'row> &'row str: ColumnIndex<<S::Database as Database>::Row>,
{
    let limits = crate::fleet_outbox_limits().map_err(StoreError::backend)?;
    let binding = crate::fleet_sender_binding::FleetSenderBinding;
    let expired =
        fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(scope, limits, &binding)
            .expired_pushes(now, 100)
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?;
    for item in expired {
        let subject = item.as_uuid();
        let sent = crate::work_attempts::attempt(scope, subject, AttemptKind::Push)
            .await?
            .is_some_and(|row| {
                row.attempts > 0 || matches!(row.state, AttemptState::InFlight { .. })
            });
        let reach = if sent {
            durable_steps::storage::CallReach::MayHaveReached
        } else {
            durable_steps::storage::CallReach::NeverSent
        };
        fleet_mail_delivery::Outbox::<_, BoardDeliveryTypes, _>::new(scope, limits, &binding)
            .finish_push_at_deadline::<CentralMailSink>(item, reach, now)
            .await
            .map_err(fleet_mail_delivery::delivery_rollback)?;
        crate::work_attempts::succeeded(scope, subject, AttemptKind::Push).await?;
    }
    Ok(())
}
