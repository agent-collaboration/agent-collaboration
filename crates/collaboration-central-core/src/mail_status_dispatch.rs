//! The status stream's peer hop is local socket I/O, never an object call.

use crate::{
    ObjectHost, PlatformIo,
    holder_reply_hub::HolderReplyHub,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::BoardDeliveryTypes;
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallId, StoreError, storage::TransactionScope,
};
use fleet_sync_delivery::{PublicationRef, PublicationTarget, PublishCall, resolve_publication};
use fleet_sync_protocol::{
    DeliveryFailureKind, DeliveryRefusal, ServerCall, StatusPublication, StatusPublicationResult,
};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    sync::{Arc, Weak},
    time::Duration,
};

type Publication =
    StatusPublication<<BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance>;
pub(crate) type PublicationDispatcher = Arc<
    dyn Fn(
            PublishCall<BoardDeliveryTypes>,
            Publication,
            CallId,
        ) -> BoxFuture<'static, Result<StatusPublicationResult, CallFailure>>
        + Send
        + Sync,
>;

pub(crate) fn bind<Host, Io>(
    host: Weak<Host>,
    io: Arc<Io>,
    replies: Arc<HolderReplyHub>,
    installation: ServiceId,
    timeout: Duration,
) -> PublicationDispatcher
where
    Host: ObjectHost,
    Io: PlatformIo,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    Arc::new(move |target, publication, call| {
        let host = host.clone();
        let io = io.clone();
        let replies = replies.clone();
        let installation = installation.clone();
        Box::pin(async move {
            let PublicationTarget::Peer { peer, route } = &target.target else {
                return Ok(invalid_route());
            };
            if route != &installation {
                return Ok(invalid_route());
            }
            let peer = *peer;
            let host = host.upgrade().ok_or(CallFailure::AnswerLost)?;
            let reference = PublicationRef {
                item: publication.item,
                version: publication.version,
                target: target.target,
            };
            // The shared append commits this exact owed publication together
            // with its immutable stream event. Resolve through that owner.
            let (stored, _) = StepDriver::new(host.clone())
                .run(ResolveStatus(reference))
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
            let Some(stored) = stored else {
                // A newer owed revision superseded this task's saved local read.
                return Ok(StatusPublicationResult::Applied);
            };
            if stored != publication {
                return Ok(invalid_route());
            }
            let attempt = crate::peer_request::begin_peer_request(
                host,
                io,
                replies,
                peer,
                ServerCall::PublishStatus { call, publication },
                timeout,
            )
            .await;
            // Applied means stored and fetchable. Waiting here for the Router
            // would also occupy the alarm drive which expires its reply waiter.
            drop(attempt);
            Ok(StatusPublicationResult::Applied)
        })
    })
}

pub(crate) fn invalid_route() -> StatusPublicationResult {
    StatusPublicationResult::Refused {
        reason: DeliveryRefusal::ProcessingFailed {
            kind: DeliveryFailureKind::InvalidPayload,
        },
    }
}

struct ResolveStatus(PublicationRef<ServiceId>);
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ResolveStatus
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<Publication>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let publication = resolve_publication::<_, BoardDeliveryTypes>(scope, self.0)
                .await
                .map_err(StoreError::backend)?;
            Ok((publication, Vec::new()))
        })
    }
}
