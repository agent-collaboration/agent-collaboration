//! Named points a scenario test can observe or hold (design note §11.8).
//! Compiled only with the test-support feature: production builds have no
//! hook and no call sites. A test installs a hook, for example one that
//! forwards each point to Turmoil's barriers, so Turmoil stays a
//! dev-dependency.

use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;

/// A named point in the client's own code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientSimPoint {
    /// The owner offered the dispatch mark and is awaiting its settlement.
    DispatchMarkOffered,
    /// The reader reported a run loss. Its stamped event hasn't reached the owner.
    RunLossReady,
}

/// What a test runs at each named point; the client awaits it.
pub type SimPointHook = fn(ClientSimPoint) -> Pin<Box<dyn Future<Output = ()> + Send>>;

static HOOK: OnceLock<SimPointHook> = OnceLock::new();

/// Installs the process's hook once; a second install keeps the first.
pub fn install(hook: SimPointHook) {
    let _kept = HOOK.set(hook);
}

/// The client reached `point`: the installed hook runs, if there is one.
pub async fn reach(point: ClientSimPoint) {
    if let Some(hook) = HOOK.get() {
        hook(point).await;
    }
}
