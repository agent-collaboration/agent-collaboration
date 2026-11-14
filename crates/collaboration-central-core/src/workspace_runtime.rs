//! One org's request owner. Stores execute statements; only this core makes decisions.
use crate::{ObjectHost, WorkspaceTurns};
use central_storage::{
    BatchOutcome, ReadOutcome, Statement, StatementBatch, StoreCall, StoreFailure, StoreReply,
};
use fleet_identity_protocol::OrganizationId;
use runtime_primitives::{Clock, IdSource};
use std::{future::Future, sync::Arc};
use workspace_model::{Caller, WorkspaceId};

pub trait WorkspaceStoreCalls: Clone + Send + Sync + 'static {
    fn call_workspace(
        &self,
        workspace: &WorkspaceId,
        call: StoreCall,
    ) -> impl Future<Output = Result<StoreReply, StoreFailure>> + Send;
}
/// Transport-stamped context. No workspace allowance is accepted from the caller.
#[derive(Clone)]
pub struct WorkspaceCaller {
    pub organization_id: OrganizationId,
    pub caller: Caller,
    pub credential_hash: String,
}

pub struct WorkspaceRuntime<Host: ObjectHost, Stores: WorkspaceStoreCalls> {
    pub(crate) organization: OrganizationId,
    pub(crate) host: Arc<Host>,
    pub(crate) stores: Stores,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdSource>,
    pub(crate) turns: WorkspaceTurns,
    pub(crate) central_turn: tokio::sync::Mutex<()>,
    #[cfg(feature = "test-support")]
    central_waiting: tokio::sync::watch::Sender<usize>,
    pub(crate) mail_policy: collaboration_protocol::central_mail::MailPolicy,
    pub(crate) drain_retry_seconds: std::sync::Mutex<std::collections::BTreeMap<WorkspaceId, u64>>,
    #[cfg(feature = "test-support")]
    pub(crate) pauses: crate::PausePoints,
    #[cfg(feature = "test-support")]
    lost_answers: std::sync::Mutex<std::collections::BTreeSet<workspace_model::RequestId>>,
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub fn new(
        organization: OrganizationId,
        host: Arc<Host>,
        stores: Stores,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
    ) -> Self {
        Self {
            organization,
            host,
            stores,
            clock,
            ids,
            turns: WorkspaceTurns::default(),
            central_turn: tokio::sync::Mutex::new(()),
            #[cfg(feature = "test-support")]
            central_waiting: tokio::sync::watch::channel(0).0,
            mail_policy: Default::default(),
            drain_retry_seconds: Default::default(),
            #[cfg(feature = "test-support")]
            pauses: crate::PausePoints::default(),
            #[cfg(feature = "test-support")]
            lost_answers: Default::default(),
        }
    }
    pub fn with_mail_policy(
        mut self,
        policy: collaboration_protocol::central_mail::MailPolicy,
    ) -> Self {
        self.mail_policy = policy;
        self
    }
    pub fn statement_host(&self) -> &Host {
        &self.host
    }
    pub(crate) async fn take_central_turn(&self) -> tokio::sync::MutexGuard<'_, ()> {
        #[cfg(not(feature = "test-support"))]
        {
            self.central_turn.lock().await
        }
        #[cfg(feature = "test-support")]
        {
            use std::future::Future;
            let mut acquisition = std::pin::pin!(self.central_turn.lock());
            let mut waiting = None;
            std::future::poll_fn(|context| match acquisition.as_mut().poll(context) {
                std::task::Poll::Pending => {
                    waiting
                        .get_or_insert_with(|| CentralWaiter::enter(self.central_waiting.clone()));
                    std::task::Poll::Pending
                }
                ready @ std::task::Poll::Ready(_) => ready,
            })
            .await
        }
    }
    #[cfg(feature = "test-support")]
    pub async fn until_central_waiting(&self, count: usize) {
        let mut waiting = self.central_waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
    pub(crate) fn mint<T: TryFrom<String>>(&self) -> Result<T, StoreFailure> {
        T::try_from(self.ids.new_id().to_string())
            .map_err(|_| crate::workspace_decode_error::corrupt("minted id"))
    }
    pub(crate) fn now(&self) -> Result<workspace_model::Timestamp, StoreFailure> {
        chrono::DateTime::from_timestamp_millis(self.clock.now().unix_millis())
            .ok_or_else(|| crate::workspace_decode_error::corrupt("clock"))
    }
    pub(crate) async fn read_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: Statement,
    ) -> Result<ReadOutcome, StoreFailure> {
        match self
            .stores
            .call_workspace(workspace, StoreCall::Read(statement))
            .await?
        {
            StoreReply::Read(rows) => Ok(rows),
            StoreReply::Batch(_) => Err(crate::workspace_decode_error::corrupt("store reply")),
        }
    }
    pub(crate) async fn batch_workspace(
        &self,
        workspace: &WorkspaceId,
        batch: StatementBatch,
    ) -> Result<BatchOutcome, StoreFailure> {
        match self
            .stores
            .call_workspace(workspace, StoreCall::Batch(batch))
            .await?
        {
            StoreReply::Batch(outcome) => Ok(outcome),
            StoreReply::Read(_) => Err(crate::workspace_decode_error::corrupt("store reply")),
        }
    }
    #[cfg(feature = "test-support")]
    pub fn pause_points(&self) -> crate::PausePoints {
        self.pauses.clone()
    }
    #[cfg(feature = "test-support")]
    pub fn workspace_turns(&self) -> &WorkspaceTurns {
        &self.turns
    }
    #[cfg(feature = "test-support")]
    pub fn lose_reply_after_apply(&self, request: workspace_model::RequestId) {
        self.lost_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(request);
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn take_lost_reply(&self, request: &workspace_model::RequestId) -> bool {
        self.lost_answers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(request)
    }
    #[cfg(feature = "test-support")]
    pub async fn inspect_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: Statement,
    ) -> Result<ReadOutcome, StoreFailure> {
        self.read_workspace(workspace, statement).await
    }
}

#[cfg(feature = "test-support")]
struct CentralWaiter(tokio::sync::watch::Sender<usize>);
#[cfg(feature = "test-support")]
impl CentralWaiter {
    fn enter(waiting: tokio::sync::watch::Sender<usize>) -> Self {
        waiting.send_modify(|count| *count += 1);
        Self(waiting)
    }
}
#[cfg(feature = "test-support")]
impl Drop for CentralWaiter {
    fn drop(&mut self) {
        self.0.send_modify(|count| *count -= 1);
    }
}
