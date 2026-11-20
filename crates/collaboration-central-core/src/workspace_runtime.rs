//! Organization feature composition over one Fleet runtime.
use crate::MailDeliveryReady;
use fleet_runtime::FleetRuntime;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_workspace_access::WorkspaceAccess;
use std::sync::Arc;

/// Composes one Fleet runtime with the retained organization features.
pub struct WorkspaceRuntime<Host: CentralStore, Stores: WorkspaceStores> {
    pub(crate) execution: FleetRuntime<Host, Stores>,
    pub(crate) mail_delivery_ready: Arc<dyn Fn(MailDeliveryReady) + Send + Sync>,
    pub(crate) machine_revoked: Arc<dyn Fn(workspace_ids::MachineId) + Send + Sync>,
    pub(crate) signin_authority: crate::organization_authority_cache::OrganizationAuthority,
    #[cfg(feature = "test-support")]
    lost_provisioning_reply: std::sync::atomic::AtomicBool,
    #[cfg(feature = "test-support")]
    pub(crate) machine_restore_failure:
        std::sync::Mutex<Option<(usize, central_storage::StoreFailure)>>,
}
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub fn new(execution: FleetRuntime<Host, Stores>) -> Self {
        Self {
            execution,
            signin_authority: Default::default(),
            mail_delivery_ready: Arc::new(|_| {}),
            machine_revoked: Arc::new(|_| {}),
            #[cfg(feature = "test-support")]
            lost_provisioning_reply: Default::default(),
            #[cfg(feature = "test-support")]
            machine_restore_failure: Default::default(),
        }
    }

    pub fn runtime(&self) -> &FleetRuntime<Host, Stores> {
        &self.execution
    }

    pub(crate) fn access(&self) -> WorkspaceAccess<'_, Host, Stores> {
        WorkspaceAccess::new(&self.execution)
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
}
