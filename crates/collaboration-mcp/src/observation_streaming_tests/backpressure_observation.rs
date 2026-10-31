//! A real Pending notification send identifies the stalled-reader condition.
use tokio::sync::watch;
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct BackpressureLayer(watch::Sender<bool>);
impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for BackpressureLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() == "collaboration_mcp::runtime_test_observation_backpressure" {
            self.0.send_replace(true);
        }
    }
}

pub(super) fn observe_pending() -> (impl tracing::Subscriber, watch::Receiver<bool>) {
    let (pending, observed) = watch::channel(false);
    (
        tracing_subscriber::registry().with(BackpressureLayer(pending)),
        observed,
    )
}
