//! Observe only the exact synthetic forwarded request entering the real holder.
use message_board::RequestId;
use tokio::sync::watch;
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct RequestField<'request> {
    expected: &'request str,
    matched: bool,
}
impl Visit for RequestField<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "request_id" && value == self.expected {
            self.matched = true;
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

struct ForwardedRequestLayer {
    expected: String,
    received: watch::Sender<bool>,
}
impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for ForwardedRequestLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() != "codex_router_host::runtime_test_forwarded_post" {
            return;
        }
        let mut request = RequestField {
            expected: &self.expected,
            matched: false,
        };
        event.record(&mut request);
        if request.matched {
            self.received.send_replace(true);
        }
    }
}

pub(super) fn observe_request(
    request: &RequestId,
) -> (impl tracing::Subscriber, watch::Receiver<bool>) {
    let (received, observed) = watch::channel(false);
    (
        tracing_subscriber::registry().with(ForwardedRequestLayer {
            expected: request.as_str().to_owned(),
            received,
        }),
        observed,
    )
}
