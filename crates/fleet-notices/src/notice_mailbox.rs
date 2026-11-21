//! The queue operations Core supplies to Notices from Mail.
use central_storage::{GuardedStatement, StoreFailure};
use fleet_contracts::MailDeliveryReady;
use std::future::Future;
use workspace_ids::SessionRef;
use workspace_model::{
    ActorIdentityId, ThreadId, Timestamp, WorkspaceId,
    central_mail::{NoticeItem, NoticeSource},
};

pub trait NoticeMailbox: Sync {
    /// The caller holds the central turn for the notice planning batch.
    fn validate_queued_notices_held(&self)
    -> impl Future<Output = Result<(), StoreFailure>> + Send;

    /// The caller holds the central turn for validation, planning and application.
    fn plan_cancel_queued_notices_held<'a>(
        &'a self,
        workspace: &'a WorkspaceId,
        thread: &'a ThreadId,
        recipient: Option<&'a ActorIdentityId>,
        now: Timestamp,
    ) -> impl Future<Output = Result<Vec<GuardedStatement>, StoreFailure>> + Send + 'a;

    fn plan_sourced_notice(
        &self,
        target: SessionRef,
        items: Vec<NoticeItem>,
        source: NoticeSource,
        now: Timestamp,
    ) -> Result<(GuardedStatement, MailDeliveryReady), StoreFailure>;
}
