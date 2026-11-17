//! A provider connection's runtime as a session line holds it: the line
//! names this handle, never the runtime type, so the FIFO stays outside the
//! runtime's module cycle (the cycle ratchet).

use super::{Arc, EndpointRef, ExternalProviderRuntime, ExternalProviderSupervisor};

/// The runtime of the connection whose session cells hold a line's
/// messages.
#[derive(Clone)]
pub(crate) struct ProviderLineRuntime(Arc<ExternalProviderRuntime>);

impl std::ops::Deref for ProviderLineRuntime {
    type Target = Arc<ExternalProviderRuntime>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ExternalProviderSupervisor {
    /// The connection runtime serving `endpoint`, as a line holds it.
    pub(crate) fn line_runtime_for(&self, endpoint: &EndpointRef) -> Option<ProviderLineRuntime> {
        self.runtime_for(endpoint).map(ProviderLineRuntime)
    }
}
