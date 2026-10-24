//! Forward slots are reserved and retired in the owning session transaction.
use super::*;
use crate::{
    forward_calls::{ForwardAnswerRequest, ForwardDeliveryRequest, ForwardRouteResult},
    forward_store::{self, ForwardPhase, PendingForward},
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, ForwardOutcome, ForwardedPost,
    VerifiedForwardedPost,
};
use fleet_sync_protocol::{FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
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
    pub(super) async fn stage_forward(
        &mut self,
        pending: PendingForward,
        post: ForwardedPost,
    ) -> Result<(), StoreError> {
        if forward_store::pending(self).await?.len() >= 16 {
            return self.stage_operation_answer(
                pending.connection,
                pending.request_frame,
                OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(
                    ForwardOutcome::Overloaded,
                )),
            );
        }
        forward_store::save_pending(self, &pending).await?;
        self.ensure_wake(pending.deadline).await?;
        self.transport
            .push(SessionTransportEffect::ForwardRoute { pending, post });
        Ok(())
    }
    pub(crate) async fn forward_route_result(
        &mut self,
        id: FrameId,
        result: Result<ForwardRouteResult, ()>,
        post: ForwardedPost,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let Some(mut pending) = forward_store::pending_by_frame(self, id).await? else {
            return Ok(());
        };
        if pending.deadline <= now || !matches!(pending.phase, ForwardPhase::Routing) {
            return Ok(());
        }
        let outcome = match result {
            Ok(ForwardRouteResult::Route {
                holder,
                installation,
                origin,
                holder_last_seen,
            }) => {
                pending.phase = ForwardPhase::AwaitingHolder {
                    installation: installation.clone(),
                };
                forward_store::save_pending(self, &pending).await?;
                let request = ForwardDeliveryRequest {
                    forward_frame: id,
                    holder,
                    deadline: pending.deadline,
                    post: VerifiedForwardedPost {
                        project: pending.project,
                        origin,
                        post,
                    },
                };
                self.transport
                    .push(SessionTransportEffect::ForwardDelivery {
                        installation,
                        request,
                        last_seen: holder_last_seen,
                    });
                return Ok(());
            }
            Ok(ForwardRouteResult::SenderHolds) => ForwardOutcome::SenderHolds,
            Ok(ForwardRouteResult::NotHeld) => ForwardOutcome::NotHeld,
            Err(()) => ForwardOutcome::Unanswered,
        };
        self.finish_forward(pending, outcome).await
    }
    pub(crate) async fn forward_delivery_failed(
        &mut self,
        id: FrameId,
        outcome: ForwardOutcome,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let Some(pending) = forward_store::pending_by_frame(self, id).await? else {
            return Ok(());
        };
        if pending.deadline <= now || !matches!(pending.phase, ForwardPhase::AwaitingHolder { .. })
        {
            return Ok(());
        }
        self.finish_forward(pending, outcome).await
    }
    pub(crate) async fn finish_forward(
        &mut self,
        pending: PendingForward,
        outcome: ForwardOutcome,
    ) -> Result<(), StoreError> {
        forward_store::remove_pending(self, pending.forward_frame).await?;
        self.transport.push(SessionTransportEffect::ForwardFrame {
            connection: pending.connection,
            session: pending.session,
            bytes: serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
                frame: pending.request_frame,
                body: OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(outcome)),
            })
            .map_err(StoreError::backend)?
            .into(),
        });
        Ok(())
    }
    pub(super) async fn stage_forward_answer(
        &mut self,
        session: SessionKey,
        frame: FrameId,
        reply: board_sync_protocol::BoardPayload,
        now: SyncTime,
    ) -> Result<(), StoreError> {
        let Some(delivered) = forward_store::delivered_by_frame(self, frame).await? else {
            return Ok(());
        };
        if delivered.deadline <= now || delivered.holder != session.peer {
            return Ok(());
        }
        forward_store::remove_delivered(self, frame).await?;
        self.transport.push(SessionTransportEffect::ForwardReply {
            installation: delivered.sender,
            request: ForwardAnswerRequest {
                forward_frame: frame,
                project: delivered.project,
                reply,
            },
        });
        Ok(())
    }
    pub(crate) async fn expire_forwards(&mut self, now: SyncTime) -> Result<(), StoreError> {
        let mut next = None;
        for pending in forward_store::pending(self).await? {
            if pending.deadline <= now {
                self.finish_forward(pending, ForwardOutcome::Unanswered)
                    .await?;
            } else {
                next = Some(next.map_or(pending.deadline, |at: SyncTime| at.min(pending.deadline)));
            }
        }
        for delivered in forward_store::delivered(self).await? {
            if delivered.deadline <= now {
                forward_store::remove_delivered(self, delivered.forward_frame).await?;
            } else {
                next = Some(next.map_or(delivered.deadline, |at| at.min(delivered.deadline)));
            }
        }
        if let Some(at) = next {
            self.ensure_wake(at).await?;
        }
        Ok(())
    }
}
