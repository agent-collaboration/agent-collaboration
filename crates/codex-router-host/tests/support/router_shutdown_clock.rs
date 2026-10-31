//! The fixture drives armed Router shutdown waits only after its real SIGTERM.
use std::future::Future;
use tokio::sync::mpsc;
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct RouterShutdownLayer(mpsc::UnboundedSender<()>);
impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for RouterShutdownLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() == "codex_router_host::runtime_test_router_shutdown" {
            let _observed = self.0.send(());
        }
    }
}

pub(crate) struct RouterShutdownClock(mpsc::UnboundedReceiver<()>);

pub(crate) fn observe_armed_deadlines() -> (impl tracing::Subscriber, RouterShutdownClock) {
    let (armed, observed) = mpsc::unbounded_channel();
    (
        tracing_subscriber::registry().with(RouterShutdownLayer(armed)),
        RouterShutdownClock(observed),
    )
}

impl RouterShutdownClock {
    pub(crate) async fn drive_until<Runtime: Future>(
        mut self,
        runtime: Runtime,
    ) -> Runtime::Output {
        tokio::pin!(runtime);
        loop {
            tokio::select! {
                result = &mut runtime => return result,
                armed = self.0.recv() => {
                    if armed.is_none() {
                        return runtime.await;
                    }
                    tokio::time::pause();
                    tokio::time::advance(codex_router_host::ROUTER_SHUTDOWN_TIMEOUT + std::time::Duration::from_millis(1)).await;
                    tokio::time::resume();
                }
            }
        }
    }
}
