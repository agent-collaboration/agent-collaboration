//! The status stream's peer hop is local socket I/O, never an object call.

use crate::{
    ObjectHost, PlatformIo,
    holder_reply_hub::HolderReplyHub,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use board_sync_protocol::{BoardConsumerProtocol, BoardDeliveryTypes};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallId, CallRetryReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_delivery::{PublicationRef, PublicationTarget, PublishCall, resolve_publication};
use fleet_sync_protocol::{
    ClientAnswer, DatabaseId, DeliveryFailureKind, DeliveryRefusal, PeerId, ServerCall,
    ServerCallAnswer, StatusPublication, StatusPublicationResult,
};
use futures_util::future::{Either, select};
use message_board::ServiceId;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{
    convert::Infallible,
    future::Future,
    sync::{Arc, Weak},
    time::Duration,
};

#[derive(Clone, Copy)]
enum StatusDestination {
    Machine(PeerId),
    Project(DatabaseId),
}

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
    timeout: Duration,
) -> PublicationDispatcher
where
    Host: ObjectHost,
    Io: PlatformIo,
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
    Arc::new(move |target, publication, call| {
        let host = host.clone();
        let io = io.clone();
        let replies = replies.clone();
        Box::pin(async move {
            let destination = match &target.target {
                PublicationTarget::Peer { peer, .. } => StatusDestination::Machine(*peer),
                PublicationTarget::Project { project, .. } => StatusDestination::Project(*project),
                PublicationTarget::StatusStream { .. } => return Ok(invalid_route()),
            };
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
            let peer = match destination {
                StatusDestination::Machine(peer) => peer,
                StatusDestination::Project(project) => {
                    let (peer, _) = StepDriver::new(host.clone())
                        .run(ReadPublicationHolder {
                            project,
                            now: io.now(),
                        })
                        .await
                        .map_err(|_| project_holder_unavailable())?;
                    peer.ok_or_else(project_holder_unavailable)?
                }
            };
            let deadline_io = io.clone();
            let attempt = crate::peer_request::begin_peer_request(
                host,
                io.clone(),
                replies,
                peer,
                ServerCall::PublishStatus { call, publication },
                timeout,
                move |remaining| async move { deadline_io.reply_deadline(remaining).await },
            )
            .await;
            match destination {
                StatusDestination::Machine(_) => {
                    // The Machine stream is already stored and fetchable, so
                    // its live hint never waits for a recipient answer.
                    drop(attempt);
                    Ok(StatusPublicationResult::Applied)
                }
                StatusDestination::Project(_) => {
                    // Project has no fetch path. Its durable call keeps the
                    // Project target and must receive the current holder's ACK.
                    let waiter = attempt?;
                    let remaining = Duration::from_millis(
                        u64::try_from(
                            waiter
                                .deadline()
                                .unix_millis()
                                .saturating_sub(io.now().unix_millis()),
                        )
                        .unwrap_or(0),
                    );
                    bounded_project_answer(waiter.answer(), io.reply_deadline(remaining)).await
                }
            }
        })
    })
}

#[cfg(test)]
#[path = "mail_status_dispatch_test.rs"]
mod tests;

async fn bounded_project_answer(
    answer: impl Future<Output = Result<ClientAnswer<BoardConsumerProtocol>, CallFailure>> + Send,
    deadline: impl Future<Output = ()> + Send,
) -> Result<StatusPublicationResult, CallFailure> {
    match select(Box::pin(answer), Box::pin(deadline)).await {
        Either::Left((Ok(ClientAnswer::Crate(ServerCallAnswer::Published(answer))), _)) => {
            Ok(answer)
        }
        Either::Left((Err(error), _)) => Err(error),
        Either::Left((Ok(_), _)) => Err(CallFailure::AnswerLost),
        // The holder may have applied the publication before losing its ACK.
        Either::Right(_) => Err(CallFailure::AnswerLost),
    }
}

fn project_holder_unavailable() -> CallFailure {
    CallFailure::Retryable {
        reason: CallRetryReason {
            code: "projectHolderUnavailable".into(),
            detail: "project has no available current holder".into(),
        },
    }
}

struct ReadPublicationHolder {
    project: DatabaseId,
    now: runtime_primitives::SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for ReadPublicationHolder
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = Option<PeerId>;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let lease = match fleet_sync_lease::lease_status(scope, self.project, self.now).await {
                Ok(lease) => lease,
                Err(fleet_sync_lease::LeaseError::Retired) => None,
                Err(error) => return Err(StoreError::backend(error)),
            };
            Ok((
                lease
                    .filter(|lease| lease.state == fleet_sync_protocol::LeaseState::Held)
                    .map(|lease| lease.holder),
                Vec::new(),
            ))
        })
    }
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
