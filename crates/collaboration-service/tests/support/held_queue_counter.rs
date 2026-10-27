//! Observe executed native SQL; no repository path is replaced.
#![expect(
    clippy::unwrap_used,
    reason = "A poisoned or uninstalled test counter must fail the diagnostic."
)]
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

#[derive(Clone, Default)]
pub struct SqlCounter(Arc<Mutex<Vec<BTreeMap<String, String>>>>);
impl SqlCounter {
    pub fn install(&self) {
        tracing_subscriber::registry()
            .with(self.clone())
            .try_init()
            .unwrap();
    }
    pub fn offset(&self) -> usize {
        self.0.lock().unwrap().len()
    }
    pub fn since(&self, offset: usize) -> Vec<BTreeMap<String, String>> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .skip(offset)
            .cloned()
            .collect()
    }
}
#[derive(Default)]
struct EventFields(BTreeMap<String, String>);
impl Visit for EventFields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.insert(field.name().into(), format!("{value:?}"));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.0.insert(field.name().into(), value.into());
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.0.insert(field.name().into(), value.to_string());
    }
}
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for SqlCounter {
    fn on_event(&self, event: &tracing::Event<'_>, _: Context<'_, TSubscriber>) {
        if event.metadata().target() == "sqlx::query" {
            let mut fields = EventFields::default();
            event.record(&mut fields);
            self.0.lock().unwrap().push(fields.0);
        }
    }
}
