//! Host supplies persistence; service workers share only the current admitted configuration.
use collaboration_protocol::{
    AutomationConfiguration, AutomationConfigureRequest, ConfigurationFailure,
};
use std::{future::Future, pin::Pin, sync::Arc};
use tokio::sync::{RwLock, watch};
#[derive(Clone)]
pub struct AutomationConfigurationHandle {
    state: Arc<RwLock<ConfigurationState>>,
    /// Moves on every publish or suspend, so workers waiting for readiness wake on it.
    changes: watch::Sender<u64>,
}
struct ConfigurationState {
    value: AutomationConfiguration,
    ready: bool,
}
impl Default for AutomationConfigurationHandle {
    fn default() -> Self {
        Self {
            state: Arc::new(RwLock::new(ConfigurationState {
                value: AutomationConfiguration::default(),
                ready: true,
            })),
            changes: watch::channel(0).0,
        }
    }
}
pub struct ConfigurationAdmissionLease {
    state: tokio::sync::OwnedRwLockReadGuard<ConfigurationState>,
}
impl ConfigurationAdmissionLease {
    pub fn configuration(&self) -> Option<AutomationConfiguration> {
        self.state.ready.then_some(self.state.value)
    }
}
impl AutomationConfigurationHandle {
    pub async fn admission_lease(&self) -> ConfigurationAdmissionLease {
        ConfigurationAdmissionLease {
            state: Arc::clone(&self.state).read_owned().await,
        }
    }
    pub async fn current(&self) -> Option<AutomationConfiguration> {
        let state = self.state.read().await;
        state.ready.then_some(state.value)
    }
    pub async fn suspend(&self) {
        self.state.write().await.ready = false;
        self.changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    pub async fn publish(&self, value: AutomationConfiguration) {
        {
            let mut state = self.state.write().await;
            state.value = value;
            state.ready = true;
        }
        self.changes
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
    /// A receiver that wakes after each publish or suspend.
    #[must_use]
    pub fn subscribe_changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }
}
pub trait AutomationConfigurationBackend: Send + Sync {
    /// Recover only this configuration receipt; completed older operations never restore old settings.
    fn reconcile(
        &self,
        operation_id: collaboration_protocol::OperationId,
    ) -> Pin<Box<dyn Future<Output = Result<(), ConfigurationFailure>> + Send + '_>>;
    fn configure(
        &self,
        request: AutomationConfigureRequest,
    ) -> Pin<
        Box<dyn Future<Output = Result<AutomationConfiguration, ConfigurationFailure>> + Send + '_>,
    >;
}
