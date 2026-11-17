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
    /// A close reached its wait for the session's turn end.
    CloseTurnWait,
    /// A prompt's turn is open on the session's cell; its command hasn't
    /// reached the connection yet.
    PromptTurnOpen,
    /// A session's actor ended on its own (its command channel closed); the
    /// hand-off of its exit to the session's cell and the hub hasn't finished.
    ActorExitHandoff,
}

/// What a test runs at each named point; the client awaits it.
pub type SimPointHook = fn(ClientSimPoint) -> Pin<Box<dyn Future<Output = ()> + Send>>;

static HOOK: OnceLock<SimPointHook> = OnceLock::new();

/// Installs the process's hook once; a second install keeps the first.
pub fn install(hook: SimPointHook) {
    let _kept = HOOK.set(hook);
}

/// The client reached `point`: the installed hook runs, if there is one.
pub(crate) async fn reach(point: ClientSimPoint) {
    if let Some(hook) = HOOK.get() {
        hook(point).await;
    }
}
