//! Observe the real ACP owner's decision for one exact approval request.
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

struct WithheldInteractionLayer {
    expected: String,
    withheld: watch::Sender<bool>,
}

impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for WithheldInteractionLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target()
            != "collaboration_runtime::runtime_test_acp_interaction_withheld"
        {
            return;
        }
        let mut request = RequestField {
            expected: &self.expected,
            matched: false,
        };
        event.record(&mut request);
        if request.matched {
            self.withheld.send_replace(true);
        }
    }
}

pub(super) fn observe_request(
    request_id: &str,
) -> (impl tracing::Subscriber, watch::Receiver<bool>) {
    let (withheld, observed) = watch::channel(false);
    (
        tracing_subscriber::registry().with(WithheldInteractionLayer {
            expected: request_id.to_owned(),
            withheld,
        }),
        observed,
    )
}
