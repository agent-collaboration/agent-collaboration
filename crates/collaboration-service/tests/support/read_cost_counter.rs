//! Observe real SQLx statements and storage decodes without replacing either path.
#![allow(clippy::unwrap_used, clippy::panic)] // A broken test counter must fail the proof.
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
};
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
#[path = "sql_cost_classification.rs"]
mod sql_cost_classification;
use sql_cost_classification::StatementTables;
#[path = "alarm_read_costs.rs"]
mod alarm_read_costs;
#[allow(unused_imports)] // Only operation-cost proof targets consume observations.
pub use alarm_read_costs::{AlarmRunCosts, observe_alarm_costs};

const CORE_CANONICAL_INPUT: &str =
    include_str!("../../../collaboration-central-core/sql/fleet/read_canonical_fleet_record.sql");
const CORE_FEED_INPUT: &str =
    include_str!("../../../collaboration-central-core/sql/fleet/read_fleet_feed_page.sql");
const CORE_EXPIRED_FEED_INPUT: &str =
    include_str!("../../../collaboration-central-core/sql/fleet/read_expired_fleet_feed.sql");

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ReadCost {
    pub statements: u64,
    pub project_opens: u64,
    pub records: u64,
    pub participants: u64,
    pub feed_entries: u64,
    pub trail_metadata: u64,
    pub core_canonical_input_bodies: u64,
    pub core_feed_input_bodies: u64,
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
            core_canonical_input_bodies: self.core_canonical_input_bodies
                - before.core_canonical_input_bodies,
            core_feed_input_bodies: self.core_feed_input_bodies - before.core_feed_input_bodies,
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

#[derive(Debug, Default)]
struct ObservedSqlQuery {
    statement: String,
    summary: String,
    rows_returned: u64,
}
impl Visit for ObservedSqlQuery {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "db.statement" => self.statement = value.to_owned(),
            "summary" => self.summary = value.to_owned(),
            _ => {}
        }
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == "rows_returned" {
            self.rows_returned = value;
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
fn only_library_bookkeeping(statement: &str) -> bool {
    static CLASSIFIED: OnceLock<Mutex<BTreeMap<String, bool>>> = OnceLock::new();
    *CLASSIFIED
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .unwrap()
        .entry(statement.to_owned())
        .or_insert_with(|| {
            StatementTables::parse(statement)
                .is_some_and(|tables| tables.only_library_bookkeeping())
        })
}
fn statements() -> &'static Mutex<Vec<String>> {
    static STATEMENTS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    STATEMENTS.get_or_init(|| Mutex::new(Vec::new()))
}
#[allow(dead_code)] // Only query-plan proof targets consume the captured SQL.
pub fn observed_statements() -> Vec<String> {
    statements().lock().unwrap().clone()
}
struct StatementField(String);
impl Visit for StatementField {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "db.statement" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "db.statement" {
            self.0 = format!("{value:?}");
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
                let mut query = ObservedSqlQuery::default();
                event.record(&mut query);
                let statement = if query.statement.is_empty() {
                    query.summary.as_str()
                } else {
                    query.statement.as_str()
                };
                // The fixture's live library alarm runner owns journal work
                // outside the measured board operation. Only exclusively
                // _fleet_sync_* tables are omitted (Lead 894/896); mixed queries,
                // unknown SQL and every board/cache read remain in the budget.
                // This bookkeeping is per operation rather than per board row.
                if only_library_bookkeeping(statement) {
                    return;
                }
                // These are input rows at the actual Core query boundaries,
                // not observations of how often serde invokes a decoder.
                if statement.trim() == CORE_CANONICAL_INPUT.trim() {
                    cost.core_canonical_input_bodies += query.rows_returned;
                }
                if [CORE_FEED_INPUT, CORE_EXPIRED_FEED_INPUT]
                    .iter()
                    .any(|input| statement.trim() == input.trim())
                {
                    cost.core_feed_input_bodies += query.rows_returned;
                }
                cost.statements += 1;
                let mut statement = StatementField(String::new());
                event.record(&mut statement);
                if !statement.0.is_empty() {
                    statements().lock().unwrap().push(statement.0);
                }
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
            alarm_read_costs::AlarmReadCostLayer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("sqlx::query", tracing::Level::TRACE)
                    .with_target(
                        "collaboration_central_tokio::native_objects",
                        tracing::Level::TRACE,
                    ),
            ),
        )
        .with(
            ReadCostLayer.with_filter(
                tracing_subscriber::filter::Targets::new()
                    .with_target("sqlx::query", tracing::Level::TRACE)
                    .with_target(
                        "collaboration_central_tokio::native_objects",
                        tracing::Level::TRACE,
                    )
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
                    )
                    .with_target(
                        "message_board_storage::project_read_store",
                        tracing::Level::TRACE,
                    ),
            ),
        )
        .try_init()
        .unwrap();
}
