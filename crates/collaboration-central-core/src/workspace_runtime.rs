//! One org's request owner. Stores execute statements; only this core makes decisions.
use crate::{MailDeliveryReady, ObjectHost, WorkspaceTurns};
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
/// A semantic caller bound to core-validated machine_id provenance.
#[derive(Clone)]
pub struct WorkspaceCaller {
    connection: crate::ValidatedMachineConnection,
    caller: Caller,
}
impl WorkspaceCaller {
    pub fn new(
        connection: crate::ValidatedMachineConnection,
        caller: Caller,
    ) -> Result<Self, workspace_model::RefusalReason> {
        if caller.machine_id() != connection.machine_id() {
            return Err(workspace_model::RefusalReason::MachineRevoked);
        }
        Ok(Self { connection, caller })
    }
    /// A public machine link carries only sessions bound to its validated machine.
    pub fn from_machine_link(
        connection: crate::ValidatedMachineConnection,
        caller: Caller,
    ) -> Result<Self, workspace_model::RefusalReason> {
        if !matches!(caller, Caller::Session(_)) {
            return Err(workspace_model::RefusalReason::InvalidInput {
                field: "caller".to_owned(),
            });
        }
        Self::new(connection, caller)
    }

    #[must_use]
    pub fn organization_id(&self) -> &OrganizationId {
        self.connection.organization_id()
    }
    #[must_use]
    pub fn caller(&self) -> &Caller {
        &self.caller
    }
    #[must_use]
    pub fn connection(&self) -> &crate::ValidatedMachineConnection {
        &self.connection
    }
}

#[cfg(feature = "test-support")]
pub use crate::engine::port::LegacyUnitInvocations;

/// One organization's request runtime. Machine grant lifetime is fixed by central
/// policy and has no public configuration API.
pub struct WorkspaceRuntime<Host: ObjectHost, Stores: WorkspaceStoreCalls> {
    pub(crate) organization: OrganizationId,
    pub(crate) host: Arc<Host>,
    pub(crate) stores: Stores,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Arc<dyn IdSource>,
    pub(crate) due_batches: Arc<crate::DueBatchState>,
    pub(crate) turns: WorkspaceTurns,
    pub(crate) signin_observer: Arc<dyn Fn(crate::CentralObservation) + Send + Sync>,
    pub(crate) mail_delivery_ready: Arc<dyn Fn(MailDeliveryReady) + Send + Sync>,
    pub(crate) machine_revoked: Arc<dyn Fn(workspace_ids::MachineId) + Send + Sync>,
    pub(crate) signin_authority: crate::organization_authority_cache::OrganizationAuthority,
    pub(crate) central_turn: tokio::sync::Mutex<()>,
    #[cfg(feature = "test-support")]
    pub(crate) completed_alarms: tokio::sync::watch::Sender<u64>,
    /// Test-support record of every alarm drive that failed, in order.
    #[cfg(feature = "test-support")]
    failed_alarms: tokio::sync::watch::Sender<Vec<String>>,
    #[cfg(feature = "test-support")]
    lost_provisioning_reply: std::sync::atomic::AtomicBool,
    #[cfg(feature = "test-support")]
    central_waiting: tokio::sync::watch::Sender<usize>,
    /// Test-support count of machine admissions waiting on the central turn,
    /// kept apart from every other wait on it.
    #[cfg(feature = "test-support")]
    admission_waiting: tokio::sync::watch::Sender<usize>,
    #[cfg(feature = "test-support")]
    pub(crate) pauses: crate::PausePoints,
    #[cfg(feature = "test-support")]
    pub(crate) legacy_invocations: std::sync::Mutex<LegacyUnitInvocations>,
    #[cfg(feature = "test-support")]
    lost_answers: std::sync::Mutex<std::collections::BTreeSet<workspace_model::RequestId>>,
    #[cfg(feature = "test-support")]
    pub(crate) machine_restore_failure: std::sync::Mutex<Option<(usize, StoreFailure)>>,
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub fn new(
        organization: OrganizationId,
        host: Arc<Host>,
        stores: Stores,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdSource>,
        due_batches: Arc<crate::DueBatchState>,
    ) -> Self {
        Self {
            organization,
            host,
            stores,
            clock,
            ids,
            due_batches,
            turns: WorkspaceTurns::default(),
            central_turn: tokio::sync::Mutex::new(()),
            #[cfg(feature = "test-support")]
            completed_alarms: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            failed_alarms: tokio::sync::watch::channel(Vec::new()).0,
            #[cfg(feature = "test-support")]
            legacy_invocations: Default::default(),
            signin_authority: Default::default(),
            signin_observer: Arc::new(|_| {}),
            mail_delivery_ready: Arc::new(|_| {}),
            machine_revoked: Arc::new(|_| {}),
            #[cfg(feature = "test-support")]
            central_waiting: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            admission_waiting: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            lost_provisioning_reply: Default::default(),
            #[cfg(feature = "test-support")]
            pauses: crate::PausePoints::default(),
            #[cfg(feature = "test-support")]
            lost_answers: Default::default(),
            #[cfg(feature = "test-support")]
            machine_restore_failure: Default::default(),
        }
    }
    #[cfg(feature = "test-support")]
    pub fn lose_next_provisioning_reply(&self) {
        self.lost_provisioning_reply
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn take_lost_provisioning_reply(&self) -> bool {
        self.lost_provisioning_reply
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }
    pub fn with_sign_in_observer(
        mut self,
        observer: Arc<dyn Fn(crate::CentralObservation) + Send + Sync>,
    ) -> Self {
        self.signin_observer = observer;
        self
    }
    pub fn with_mail_delivery_observer(
        mut self,
        observer: Arc<dyn Fn(MailDeliveryReady) + Send + Sync>,
    ) -> Self {
        self.mail_delivery_ready = observer;
        self
    }
    pub fn with_machine_revocation_observer(
        mut self,
        observer: Arc<dyn Fn(workspace_ids::MachineId) + Send + Sync>,
    ) -> Self {
        self.machine_revoked = observer;
        self
    }
    pub fn with_sign_in_verification(
        self,
        check: Arc<dyn crate::organization_authority::SignInVerification>,
    ) -> Self {
        self.signin_authority.install_check(check);
        self
    }
    pub fn statement_host(&self) -> &Host {
        &self.host
    }
    /// The due-batch start and backlog bit the next publication will use.
    #[cfg(feature = "test-support")]
    pub fn due_batch_snapshot(&self) -> crate::DueBatchSnapshot {
        self.due_batches.snapshot()
    }
    /// Every unit the due batches started, in start order (R2/R3 visit order).
    #[cfg(feature = "test-support")]
    pub fn due_unit_starts(&self) -> Vec<crate::DueUnitStart> {
        self.due_batches.unit_starts()
    }
    /// Test-support record of every claimed due-batch start, one per batch.
    #[cfg(feature = "test-support")]
    pub fn due_batch_starts(&self) -> Vec<i64> {
        self.due_batches.claimed_starts()
    }
    #[cfg(feature = "test-support")]
    pub fn legacy_unit_invocations(&self) -> LegacyUnitInvocations {
        *self
            .legacy_invocations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    #[cfg(feature = "test-support")]
    pub(crate) fn count_legacy_invocation(&self, count: impl FnOnce(&mut LegacyUnitInvocations)) {
        count(
            &mut self
                .legacy_invocations
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
    #[cfg(feature = "test-support")]
    pub fn alarm_completion_count(&self) -> u64 {
        *self.completed_alarms.borrow()
    }
    #[cfg(feature = "test-support")]
    pub fn mark_alarm_completed(&self) {
        self.completed_alarms
            .send_modify(|count| *count = count.saturating_add(1));
    }
    #[cfg(feature = "test-support")]
    pub async fn until_alarm_completed(&self, count: u64) {
        let mut completed = self.completed_alarms.subscribe();
        let _ = completed.wait_for(|current| *current >= count).await;
    }
    /// The host's alarm driver reports a failed drive (it retries at the next mark).
    #[cfg(feature = "test-support")]
    pub fn mark_alarm_failed(&self, failure: String) {
        self.failed_alarms
            .send_modify(|failures| failures.push(failure));
    }
    #[cfg(feature = "test-support")]
    pub fn alarm_failures(&self) -> Vec<String> {
        self.failed_alarms.borrow().clone()
    }
    #[cfg(feature = "test-support")]
    pub async fn until_alarm_failed(&self, count: usize) {
        let mut failed = self.failed_alarms.subscribe();
        let _ = failed.wait_for(|failures| failures.len() >= count).await;
    }
    pub(crate) async fn take_central_turn(&self) -> tokio::sync::MutexGuard<'_, ()> {
        #[cfg(not(feature = "test-support"))]
        {
            self.central_turn.lock().await
        }
        #[cfg(feature = "test-support")]
        {
            self.take_central_turn_counted(&self.central_waiting).await
        }
    }
    /// Machine admission and restoration take the same central turn; test support
    /// counts their waits separately from the work they admit.
    pub(crate) async fn take_central_turn_for_admission(&self) -> tokio::sync::MutexGuard<'_, ()> {
        #[cfg(not(feature = "test-support"))]
        {
            self.central_turn.lock().await
        }
        #[cfg(feature = "test-support")]
        {
            self.take_central_turn_counted(&self.admission_waiting)
                .await
        }
    }
    #[cfg(feature = "test-support")]
    async fn take_central_turn_counted(
        &self,
        counter: &tokio::sync::watch::Sender<usize>,
    ) -> tokio::sync::MutexGuard<'_, ()> {
        {
            use std::future::Future;
            let mut acquisition = std::pin::pin!(self.central_turn.lock());
            let mut waiting = None;
            std::future::poll_fn(|context| match acquisition.as_mut().poll(context) {
                std::task::Poll::Pending => {
                    waiting.get_or_insert_with(|| CentralWaiter::enter(counter.clone()));
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
    #[cfg(feature = "test-support")]
    pub async fn until_admission_waiting(&self, count: usize) {
        let mut waiting = self.admission_waiting.subscribe();
        let _ = waiting.wait_for(|waiting| *waiting >= count).await;
    }
    #[cfg(feature = "test-support")]
    #[must_use]
    pub fn admission_waiting_now(&self) -> usize {
        *self.admission_waiting.borrow()
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
