//! Count complete native alarm runs separately from the overlapping foreground request.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)] // A broken proof recorder must fail.
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, OnceLock},
    time::Duration,
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    span::{Attributes, Id},
};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};

struct AlarmRun {
    object_kind: String,
    statements: u64,
    completed: bool,
}
#[derive(Default)]
struct AlarmState {
    next_id: u64,
    statements: u64,
    runs: BTreeMap<u64, AlarmRun>,
}
fn state() -> &'static Mutex<AlarmState> {
    static STATE: OnceLock<Mutex<AlarmState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AlarmState::default()))
}
fn completed_runs() -> &'static tokio::sync::watch::Sender<u64> {
    static COMPLETED: OnceLock<tokio::sync::watch::Sender<u64>> = OnceLock::new();
    COMPLETED.get_or_init(|| tokio::sync::watch::channel(0).0)
}
struct AlarmRunId(u64);
#[derive(Default)]
struct ObjectKind(String);
impl Visit for ObjectKind {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "object_kind" {
            self.0 = value.to_owned();
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
pub(super) struct AlarmReadCostLayer;
impl<TSubscriber> Layer<TSubscriber> for AlarmReadCostLayer
where
    TSubscriber: Subscriber + for<'lookup> LookupSpan<'lookup>,
{
    fn on_new_span(&self, attributes: &Attributes<'_>, id: &Id, context: Context<'_, TSubscriber>) {
        if attributes.metadata().name() != "central_alarm" {
            return;
        }
        let mut object_kind = ObjectKind::default();
        attributes.record(&mut object_kind);
        assert!(
            !object_kind.0.is_empty(),
            "alarm span must identify its object face"
        );
        let run_id = {
            let mut state = state().lock().unwrap();
            let run_id = state.next_id;
            state.next_id += 1;
            state.runs.insert(
                run_id,
                AlarmRun {
                    object_kind: object_kind.0,
                    statements: 0,
                    completed: false,
                },
            );
            run_id
        };
        context
            .span(id)
            .unwrap()
            .extensions_mut()
            .insert(AlarmRunId(run_id));
    }
    fn on_event(&self, event: &Event<'_>, context: Context<'_, TSubscriber>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut query = super::ObservedSqlQuery::default();
        event.record(&mut query);
        let statement = if query.statement.is_empty() {
            &query.summary
        } else {
            &query.statement
        };
        if super::only_library_bookkeeping(statement) {
            return;
        }
        let run_id = context.event_scope(event).and_then(|mut scope| {
            scope.find_map(|span| span.extensions().get::<AlarmRunId>().map(|id| id.0))
        });
        if let Some(run_id) = run_id {
            let mut state = state().lock().unwrap();
            state.statements += 1;
            state
                .runs
                .get_mut(&run_id)
                .expect("registered alarm run")
                .statements += 1;
        }
    }
    fn on_close(&self, id: Id, context: Context<'_, TSubscriber>) {
        let run_id = context
            .span(&id)
            .and_then(|span| span.extensions().get::<AlarmRunId>().map(|id| id.0));
        if let Some(run_id) = run_id {
            state()
                .lock()
                .unwrap()
                .runs
                .get_mut(&run_id)
                .expect("registered alarm run")
                .completed = true;
            completed_runs().send_modify(|revision| *revision += 1);
        }
    }
}

pub struct AlarmCostObservation {
    first_new_run: u64,
    already_running: BTreeSet<u64>,
    statements_before: u64,
}
#[derive(Debug)]
pub struct AlarmRunCosts {
    /// All alarm SQL in the same window as the foreground cost counter.
    pub overlapping_statements: u64,
    /// Complete runs, including a run that straddled either observation boundary.
    pub by_object: BTreeMap<String, Vec<u64>>,
}
impl AlarmRunCosts {
    pub fn distinct_costs(&self) -> BTreeMap<String, BTreeSet<u64>> {
        self.by_object
            .iter()
            .map(|(kind, counts)| (kind.clone(), counts.iter().copied().collect()))
            .collect()
    }
}
pub fn observe_alarm_costs() -> AlarmCostObservation {
    let state = state().lock().unwrap();
    AlarmCostObservation {
        first_new_run: state.next_id,
        already_running: state
            .runs
            .iter()
            .filter_map(|(id, run)| (!run.completed).then_some(*id))
            .collect(),
        statements_before: state.statements,
    }
}
impl AlarmCostObservation {
    pub async fn finish(self) -> Result<AlarmRunCosts, Box<dyn std::error::Error + Send + Sync>> {
        let mut completed = completed_runs().subscribe();
        let (overlapping_statements, selected) = {
            let state = state().lock().unwrap();
            let selected = state
                .runs
                .keys()
                .filter(|id| **id >= self.first_new_run || self.already_running.contains(id))
                .copied()
                .collect::<Vec<_>>();
            (state.statements - self.statements_before, selected)
        };
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                completed.borrow_and_update();
                let by_object = {
                    let state = state().lock().unwrap();
                    let runs = selected
                        .iter()
                        .map(|id| state.runs.get(id).expect("observed alarm run"))
                        .collect::<Vec<_>>();
                    if runs.iter().all(|run| run.completed) {
                        let mut by_object: BTreeMap<String, Vec<u64>> = BTreeMap::new();
                        for run in runs {
                            by_object
                                .entry(run.object_kind.clone())
                                .or_default()
                                .push(run.statements);
                        }
                        Some(by_object)
                    } else {
                        None
                    }
                };
                if let Some(by_object) = by_object {
                    return Ok(AlarmRunCosts {
                        overlapping_statements,
                        by_object,
                    });
                }
                completed
                    .changed()
                    .await
                    .map_err(|_| "alarm completion observation closed")?;
            }
        })
        .await
        .map_err(|_| "observed native alarm did not complete within the fixture bound")?
    }
}
