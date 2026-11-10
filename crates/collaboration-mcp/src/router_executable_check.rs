//! How the MCP server learns, when a client initializes, whether the running Router is stale.
use collaboration_protocol::RouterExecutableRelation;
use std::{future::Future, pin::Pin};

/// Compares the running Router with the executable now installed at its launch path, each
/// time it is asked. The Host's implementation is the same observation `host status` makes.
pub trait RouterExecutableCheck: Send + Sync {
    fn observe(&self) -> Pin<Box<dyn Future<Output = RouterExecutableRelation> + Send + '_>>;
}

/// A relation that never changes, for an API with no Router executable of its own to compare.
#[derive(Clone, Debug)]
pub struct FixedRouterExecutableRelation(pub RouterExecutableRelation);

impl RouterExecutableCheck for FixedRouterExecutableRelation {
    fn observe(&self) -> Pin<Box<dyn Future<Output = RouterExecutableRelation> + Send + '_>> {
        Box::pin(std::future::ready(self.0.clone()))
    }
}
