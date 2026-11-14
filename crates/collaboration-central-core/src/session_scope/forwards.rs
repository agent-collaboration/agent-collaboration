//! Forward slots are reserved and retired in the owning session transaction.
use super::{
    AtomicStore, ColumnIndex, CoreSessionScope, Database, Decode, Encode, IntoArguments,
    JournalScope, LegacyObjectHost, PlatformIo, ReborrowScope, SessionTransportEffect, StoreError,
    SyncTime, TransactionScope, Type,
};
use crate::{
    forward_calls::{ForwardDeliveryRequest, ForwardRouteResult},
    project_request_store::{self, ProjectRequest, ProjectRequestPhase},
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, ForwardOutcome, ForwardedPost,
    VerifiedForwardedPost,
};
use fleet_sync_protocol::{FrameId, OperationAnswer, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: LegacyObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
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
        pending: ProjectRequest,
        post: ForwardedPost,
    ) -> Result<(), StoreError> {
        if project_request_store::requests(self)
            .await?
            .iter()
            .filter(|row| row.session.peer == pending.session.peer)
            .count()
            >= 16
        {
            return self.stage_operation_answer(
                pending.connection,
                pending.request_frame,
                OperationAnswer::Consumer(BoardConsumerAnswer::ForwardedPost(
                    ForwardOutcome::Overloaded,
                )),
            );
        }
        project_request_store::save(self, &pending).await?;
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
        let Some(mut pending) = project_request_store::by_frame(self, id).await? else {
            return Ok(());
        };
        if pending.deadline <= now || !matches!(pending.phase, ProjectRequestPhase::Routing) {
            return Ok(());
        }
        let outcome = match result {
            Ok(ForwardRouteResult::Route {
                holder,
                installation,
                origin,
                holder_last_seen,
            }) => {
                pending.phase = ProjectRequestPhase::AwaitingHolder { holder };
                pending.updated_at = now;
                project_request_store::save(self, &pending).await?;
                let request = ForwardDeliveryRequest {
                    forward_frame: id,
                    holder,
                    deadline: pending.deadline,
                    origin_session: pending.session,
                    request_frame: pending.request_frame,
                    connection: pending.connection,
                    created_at: pending.created_at,
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
        let Some(pending) = project_request_store::by_frame(self, id).await? else {
            return Ok(());
        };
        if pending.deadline <= now
            || !matches!(
                pending.phase,
                ProjectRequestPhase::AwaitingHolder { .. } | ProjectRequestPhase::AtHolder { .. }
            )
        {
            return Ok(());
        }
        self.finish_forward(pending, outcome).await
    }
    pub(crate) async fn finish_forward(
        &mut self,
        pending: ProjectRequest,
        outcome: ForwardOutcome,
    ) -> Result<(), StoreError> {
        project_request_store::remove(self, pending.forward_frame).await?;
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
        let Some(request) = project_request_store::by_frame(self, frame).await? else {
            return Ok(());
        };
        if request.deadline <= now
            || !matches!(request.phase, ProjectRequestPhase::AtHolder { holder } if holder == session.peer)
        {
            return Ok(());
        }
        self.finish_forward(request, ForwardOutcome::Answered { reply })
            .await
    }
    pub(crate) async fn expire_forwards(&mut self, now: SyncTime) -> Result<(), StoreError> {
        let mut next = None;
        for pending in project_request_store::requests(self).await? {
            if pending.deadline <= now {
                self.finish_forward(pending, ForwardOutcome::Unanswered)
                    .await?;
            } else {
                next = Some(next.map_or(pending.deadline, |at: SyncTime| at.min(pending.deadline)));
            }
        }
        if let Some(at) = next {
            self.ensure_wake(at).await?;
        }
        Ok(())
    }
}
