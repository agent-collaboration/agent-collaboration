//! Observe real SQLx statements and storage decodes without replacing either path.
#![allow(clippy::unwrap_used, clippy::panic)] // A broken test counter must fail the proof.
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadCost {
    pub statements: u64,
    pub project_opens: u64,
    pub records: u64,
    pub participants: u64,
    pub feed_entries: u64,
    pub trail_metadata: u64,
}
impl ReadCost {
    pub fn since(self, before: Self) -> Self {
        Self {
            statements: self.statements - before.statements,
            project_opens: self.project_opens - before.project_opens,
            records: self.records - before.records,
            participants: self.participants - before.participants,
            feed_entries: self.feed_entries - before.feed_entries,
            trail_metadata: self.trail_metadata - before.trail_metadata,
        }
    }
}
fn counter() -> &'static Mutex<ReadCost> {
    static COUNTER: OnceLock<Mutex<ReadCost>> = OnceLock::new();
    COUNTER.get_or_init(|| Mutex::new(ReadCost::default()))
}
pub fn current_cost() -> ReadCost {
    *counter().lock().unwrap()
}
struct DecodeKind(String);
impl Visit for DecodeKind {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "decode_kind" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}
fn project_counts() -> &'static Mutex<BTreeMap<String, u64>> {
    static COUNTS: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
fn project_open_counts() -> &'static Mutex<BTreeMap<String, u64>> {
    static COUNTS: OnceLock<Mutex<BTreeMap<String, u64>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(BTreeMap::new()))
}
#[allow(dead_code)]
pub fn project_open_attempts(project: &str) -> u64 {
    project_open_counts()
        .lock()
        .unwrap()
        .get(project)
        .copied()
        .unwrap_or(0)
}
#[allow(dead_code)]
pub fn project_statements(project: &str) -> u64 {
    project_counts()
        .lock()
        .unwrap()
        .get(project)
        .copied()
        .unwrap_or(0)
}
struct ProjectField(String);
impl Visit for ProjectField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "project" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "project" {
            self.0 = format!("{value:?}");
        }
    }
}
struct ReadCostLayer;
impl<TSubscriber> Layer<TSubscriber> for ReadCostLayer
where
    TSubscriber:
        tracing::Subscriber + for<'lookup> tracing_subscriber::registry::LookupSpan<'lookup>,
{
    fn on_new_span(
        &self,
        attributes: &tracing::span::Attributes<'_>,
        id: &tracing::Id,
        context: Context<'_, TSubscriber>,
    ) {
        let mut project = ProjectField(String::new());
        attributes.record(&mut project);
        if !project.0.is_empty() {
            context.span(id).unwrap().extensions_mut().insert(project);
        }
    }
    fn on_event(&self, event: &tracing::Event<'_>, context: Context<'_, TSubscriber>) {
        let mut cost = counter().lock().unwrap();
        match event.metadata().target() {
            "message_board_storage::open" => {
                cost.project_opens += 1;
                let mut project = ProjectField(String::new());
                event.record(&mut project);
                *project_open_counts()
                    .lock()
                    .unwrap()
                    .entry(project.0)
                    .or_default() += 1;
            }
            "sqlx::query" => {
                cost.statements += 1;
                if let Some(project) = context.event_scope(event).and_then(|mut scope| {
                    scope.find_map(|span| {
                        span.extensions()
                            .get::<ProjectField>()
                            .map(|project| project.0.clone())
                    })
                }) {
                    *project_counts().lock().unwrap().entry(project).or_default() += 1;
                }
            }
            "message_board_storage::decode" => {
                let mut kind = DecodeKind(String::new());
                event.record(&mut kind);
                match kind.0.as_str() {
                    "record" => cost.records += 1,
                    "participant" => cost.participants += 1,
                    "feed" => cost.feed_entries += 1,
                    "trailMetadata" => cost.trail_metadata += 1,
                    _ => panic!("unknown decode event"),
                }
            }
            _ => {}
        }
    }
}
pub fn install() {
    tracing_subscriber::registry()
        .with(
            ReadCostLayer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("sqlx::query", tracing::Level::TRACE)
                    .with_target("message_board_storage::decode", tracing::Level::TRACE)
                    .with_target("message_board_storage::open", tracing::Level::TRACE)
                    .with_target(
                        "message_board_storage::project_store",
                        tracing::Level::TRACE,
                    )
                    .with_target(
                        "message_board_storage::project_inbox_pages",
                        tracing::Level::TRACE,
                    )
                    .with_target(
                        "message_board_storage::project_record_io",
                        tracing::Level::TRACE,
                    )
                    .with_target(
                        "message_board_storage::project_membership_io",
                        tracing::Level::TRACE,
                    ),
            ),
        )
        .try_init()
        .unwrap();
}
