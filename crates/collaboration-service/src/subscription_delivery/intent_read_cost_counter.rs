//! A subscriber scoped to one real consumer pass; unrelated parallel tests cannot affect it.
use std::sync::{Arc, Mutex};
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct IntentReadCost {
    pub statements: u64,
    pub records: u64,
}
#[derive(Clone, Default)]
pub(super) struct IntentReadCounter(Arc<Mutex<IntentReadCost>>);
impl IntentReadCounter {
    pub fn cost(&self) -> IntentReadCost {
        *self.0.lock().unwrap()
    }
    pub fn install_global(&self) {
        use tracing_subscriber::prelude::*;
        tracing_subscriber::registry()
            .with(self.clone())
            .try_init()
            .unwrap();
    }
}
impl IntentReadCost {
    pub fn since(self, before: Self) -> Self {
        Self {
            statements: self.statements - before.statements,
            records: self.records - before.records,
        }
    }
}
struct DecodeKind(String);
impl Visit for DecodeKind {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "decode_kind" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for IntentReadCounter {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        let mut cost = self.0.lock().unwrap();
        match event.metadata().target() {
            "sqlx::query" => cost.statements += 1,
            "message_board_storage::decode" => {
                let mut kind = DecodeKind(String::new());
                event.record(&mut kind);
                if kind.0 == "record" {
                    cost.records += 1;
                }
            }
            _ => {}
        }
    }
}
