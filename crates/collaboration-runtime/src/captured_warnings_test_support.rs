//! Test-only capture of the WARN events a worker emits, with its message and its `worker`
//! and `error` fields, on the test's current-thread runtime.
use std::sync::{Arc, Mutex};

/// One captured warning.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CapturedWarning {
    pub(crate) message: String,
    pub(crate) worker: String,
    pub(crate) error: String,
}

#[derive(Clone, Default)]
pub(crate) struct CapturedWarnings(Arc<Mutex<Vec<CapturedWarning>>>);

impl CapturedWarnings {
    /// Captures every WARN event on this thread until the guard drops.
    pub(crate) fn install() -> (Self, tracing::subscriber::DefaultGuard) {
        let warnings = Self::default();
        let guard =
            tracing::subscriber::set_default(tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::registry(),
                warnings.clone(),
            ));
        (warnings, guard)
    }

    pub(crate) fn events(&self) -> Vec<CapturedWarning> {
        self.0
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

impl<TSubscriber: tracing::Subscriber> tracing_subscriber::Layer<TSubscriber> for CapturedWarnings {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        if *event.metadata().level() != tracing::Level::WARN {
            return;
        }
        let mut warning = CapturedWarning::default();
        event.record(&mut warning);
        if let Ok(mut events) = self.0.lock() {
            events.push(warning);
        }
    }
}

impl tracing::field::Visit for CapturedWarning {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        match field.name() {
            "message" => value.clone_into(&mut self.message),
            "worker" => value.clone_into(&mut self.worker),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => self.message = format!("{value:?}"),
            "worker" => self.worker = format!("{value:?}").trim_matches('"').to_owned(),
            "error" => self.error = format!("{value:?}"),
            _ => {}
        }
    }
}
