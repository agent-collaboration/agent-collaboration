//! One org's request owner. Stores execute statements; only this core makes decisions.
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use std::sync::Arc;
#[cfg(feature = "test-support")]
use workspace_model::WorkspaceId;

#[cfg(feature = "test-support")]
pub use crate::engine::port::LegacyUnitInvocations;

/// One organization's request runtime. Machine grant lifetime is fixed by central
/// policy and has no public configuration API.
pub struct WorkspaceRuntime<Host: CentralStore, Stores: WorkspaceStores> {
    pub(crate) fleet_runtime: FleetRuntime<Host, Stores>,
    pub(crate) signin_authority: crate::organization_authority_cache::OrganizationAuthority,
    #[cfg(feature = "test-support")]
    pub(crate) completed_alarms: tokio::sync::watch::Sender<u64>,
    /// Test-support record of every alarm drive that failed, in order.
    #[cfg(feature = "test-support")]
    failed_alarms: tokio::sync::watch::Sender<Vec<String>>,
    #[cfg(feature = "test-support")]
    lost_provisioning_reply: std::sync::atomic::AtomicBool,
    #[cfg(feature = "test-support")]
    pub(crate) legacy_invocations: std::sync::Mutex<LegacyUnitInvocations>,
}
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub fn from_runtime(
        runtime: FleetRuntime<Host, Stores>,
        verification: Result<
            Option<Arc<dyn crate::organization_authority::SignInVerification>>,
            fleet_contracts::FleetOrganizationStatementRefusal,
        >,
    ) -> Result<Self, central_storage::StoreFailure> {
        let signin_authority =
            crate::organization_authority_cache::OrganizationAuthority::default();
        match verification {
            Ok(Some(check)) => signin_authority.install_check(check),
            Ok(None)
            | Err(fleet_contracts::FleetOrganizationStatementRefusal::InvalidConfiguration) => {}
            Err(_) => return Err(central_storage::StoreFailure::Unavailable),
        }
        Ok(Self {
            fleet_runtime: runtime,
            signin_authority,
            #[cfg(feature = "test-support")]
            completed_alarms: tokio::sync::watch::channel(0).0,
            #[cfg(feature = "test-support")]
            failed_alarms: tokio::sync::watch::channel(Vec::new()).0,
            #[cfg(feature = "test-support")]
            legacy_invocations: Default::default(),
            #[cfg(feature = "test-support")]
            lost_provisioning_reply: Default::default(),
        })
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
    pub(crate) fn workspace_directory(&self) -> impl fleet_contracts::WorkspaceDirectory + '_ {
        fleet_workspace_directory::CentralWorkspaceDirectory::new(self.statement_host())
    }

    pub fn statement_host(&self) -> &Host {
        self.fleet_runtime.statement_host()
    }
    pub(crate) fn workspace_access(
        &self,
    ) -> fleet_workspace_access::WorkspaceAccess<'_, Host, Stores> {
        fleet_workspace_access::WorkspaceAccess::new(&self.fleet_runtime)
    }
    pub fn alarm_schedule<Deadline: Copy>(&self) -> fleet_runtime::AlarmSchedule<Deadline> {
        self.fleet_runtime.alarm_schedule()
    }
    /// The due-batch start and backlog bit the next publication will use.
    #[cfg(feature = "test-support")]
    pub fn due_batch_snapshot(&self) -> fleet_runtime::DueBatchSnapshot {
        self.fleet_runtime.due_batches().snapshot()
    }
    /// Every unit the due batches started, in start order (R2/R3 visit order).
    #[cfg(feature = "test-support")]
    pub fn due_unit_starts(&self) -> Vec<fleet_runtime::DueUnitStart> {
        self.fleet_runtime.due_batches().unit_starts()
    }
    /// Test-support record of every claimed due-batch start, one per batch.
    #[cfg(feature = "test-support")]
    pub fn due_batch_starts(&self) -> Vec<i64> {
        self.fleet_runtime.due_batches().claimed_starts()
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
    #[cfg(feature = "test-support")]
    pub async fn until_central_waiting(&self, count: usize) {
        self.fleet_runtime.until_central_waiting(count).await;
    }
    #[cfg(feature = "test-support")]
    pub fn pause_points(&self) -> fleet_runtime::PausePoints {
        self.fleet_runtime.pause_points().clone()
    }
    #[cfg(feature = "test-support")]
    pub fn workspace_turns(&self) -> &fleet_runtime::WorkspaceTurns {
        self.fleet_runtime.workspace_turns()
    }

    #[cfg(feature = "test-support")]
    pub fn lose_reply_after_apply(&self, request: workspace_model::RequestId) {
        self.fleet_runtime.lose_reply_after_apply(request);
    }
    #[cfg(feature = "test-support")]
    pub async fn inspect_workspace(
        &self,
        workspace: &WorkspaceId,
        statement: central_storage::Statement,
    ) -> Result<central_storage::ReadOutcome, central_storage::StoreFailure> {
        self.fleet_runtime
            .read_workspace(workspace, statement)
            .await
    }
}
