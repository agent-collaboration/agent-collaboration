//! Calls carry bodies transiently; every state transition rechecks the original deadline.
use super::EffectDriver;
use crate::{
    ObjectHost, PlatformIo,
    forward_calls::{
        AnswerForward, DeliverForward, ForwardAnswerRequest, ForwardDeliveryRequest,
        ForwardDeliveryResult, ForwardRoute, ForwardRouteRequest,
    },
    forward_store::PendingForward,
    session_store::CoreSessionStore,
};
use board_sync_protocol::{ForwardOutcome, ForwardedPost};
use durable_steps::{AtomicStore, Host as _, ReborrowScope, storage::TransactionScope};
use message_board::ServiceId;
use runtime_primitives::Clock;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(super) fn route_forward(
        &self,
        pending: PendingForward,
        post: ForwardedPost,
    ) -> durable_steps::BoxFuture<'static, ()> {
        let driver = self.clone();
        Box::pin(async move {
            let answer = match driver.shared_host.call_identity("forward_route") {
                Ok(identity) => driver
                    .shared_host
                    .dispatch(
                        ForwardRoute,
                        ForwardRouteRequest {
                            session: pending.session,
                            project: pending.project.clone(),
                        },
                        identity,
                    )
                    .await
                    .map_err(|_| ()),
                Err(_) => Err(()),
            };
            let store = CoreSessionStore::new(
                driver.host.clone(),
                driver.shared_host.clone(),
                driver.replies.clone(),
            );
            let clock = driver.shared_host.clone();
            // A late reply cannot send or reserve a new slot. On store failure the
            // already-committed deadline remains the recovery path.
            let _ = Box::pin(store.transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .forward_route_result(pending.forward_frame, answer, post, clock.now())
                        .await
                })
            }))
            .await;
        })
    }
    pub(super) fn deliver_forward(
        &self,
        installation: ServiceId,
        request: ForwardDeliveryRequest,
        last_seen: Option<runtime_primitives::SyncTime>,
    ) -> durable_steps::BoxFuture<'static, ()> {
        let driver = self.clone();
        Box::pin(async move {
            let forward_frame = request.forward_frame;
            let answer = match driver.shared_host.call_identity("forward_delivery") {
                Ok(identity) => {
                    driver
                        .shared_host
                        .dispatch(DeliverForward { installation }, request, identity)
                        .await
                }
                Err(_) => Err(durable_steps::CallFailure::AnswerLost),
            };
            let outcome = match answer {
                Ok(ForwardDeliveryResult::Delivered) => return,
                Ok(ForwardDeliveryResult::NoConnection) => {
                    ForwardOutcome::HolderUnreachable { last_seen }
                }
                Err(_) => ForwardOutcome::Unanswered,
            };
            let store = CoreSessionStore::new(
                driver.host.clone(),
                driver.shared_host.clone(),
                driver.replies.clone(),
            );
            let clock = driver.shared_host.clone();
            let _ = Box::pin(store.transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .forward_delivery_failed(forward_frame, outcome, clock.now())
                        .await
                })
            }))
            .await;
        })
    }
    pub(super) async fn reply_forward(
        &self,
        installation: ServiceId,
        request: ForwardAnswerRequest,
    ) {
        if let Ok(identity) = self.shared_host.call_identity("forward_answer") {
            // A lost callback is intentionally not a content-bearing retry row.
            // The sender's original deadline resolves it as Unanswered.
            let _ = self
                .shared_host
                .dispatch(AnswerForward { installation }, request, identity)
                .await;
        }
    }
}
