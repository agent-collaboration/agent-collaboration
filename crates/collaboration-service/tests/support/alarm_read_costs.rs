//! Count complete native alarm runs separately from the overlapping foreground request.

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
    sql: Vec<String>,
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
    #[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
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
                    sql: Vec::new(),
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
    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    #[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
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
            let run = state.runs.get_mut(&run_id).expect("registered alarm run");
            run.statements += 1;
            run.sql.push(statement.to_owned());
        }
    }
    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    #[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
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
pub struct AlarmRunCosts {
    /// All alarm SQL in the same window as the foreground cost counter.
    pub overlapping_statements: u64,
    /// Complete runs, including a run that straddled either observation boundary.
    pub by_object: BTreeMap<String, Vec<u64>>,
    pub sql_by_object: BTreeMap<String, Vec<Vec<String>>>,
}
impl std::fmt::Debug for AlarmRunCosts {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AlarmRunCosts")
            .field("overlapping_statements", &self.overlapping_statements)
            .field("by_object", &self.by_object)
            .finish_non_exhaustive()
    }
}
const FLEET_ALARM_QUERIES: [(&str, &str); 11] = [
    (
        "feed.state",
        include_str!("../../../collaboration-central-core/sql/fleet/read_fleet_feed_state.sql"),
    ),
    (
        "feed.expired",
        include_str!("../../../collaboration-central-core/sql/fleet/read_expired_fleet_feed.sql"),
    ),
    (
        "forwards.rows",
        include_str!("../../../collaboration-central-core/sql/forwards/read_forward_rows.sql"),
    ),
    (
        "notices.peers",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_peers.sql"
        ),
    ),
    (
        "notices.state",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_state.sql"
        ),
    ),
    (
        "notices.bounds",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_bounds.sql"
        ),
    ),
    (
        "notices.expired",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_expired_machine_notices.sql"
        ),
    ),
    (
        "notices.next_expiry",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_expiry.sql"
        ),
    ),
    (
        "notices.append",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/append_machine_notice.sql"
        ),
    ),
    (
        "notices.head",
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/advance_machine_notice_head.sql"
        ),
    ),
    (
        "directory.installation",
        include_str!("../../../collaboration-central-core/sql/read_enrollments.sql"),
    ),
];

const NOTICE_APPEND_QUERY_OWNERS: [&str; 8] = [
    "notices.state",
    "notices.bounds",
    "notices.expired",
    "notices.state",
    "notices.bounds",
    "notices.append",
    "notices.head",
    "notices.next_expiry",
];

// One fresh fixture machine has no expired feed, forwards or notices. Each
// maintenance owner reads its fixed operands before variable queued work.
// SQL identities come from those owners, while
// the literal per-query counts below are the independent scaling oracle.
#[expect(
    clippy::expect_used,
    reason = "Unclassified captured SQL is an explicit test inventory failure."
)]
pub fn fixed_fleet_maintenance_profiles(
    costs: &AlarmRunCosts,
) -> BTreeSet<BTreeMap<&'static str, u64>> {
    let normalize = |sql: &str| sql.split_whitespace().collect::<Vec<_>>().join(" ");
    let queries: BTreeMap<_, _> = FLEET_ALARM_QUERIES
        .into_iter()
        .map(|(owner, sql)| (normalize(sql), owner))
        .collect();
    let expected = BTreeMap::from([
        ("feed.state", 1),
        ("feed.expired", 1),
        // SessionScope::expire_forwards reads origin and holder sides once each.
        ("forwards.rows", 2),
        ("notices.peers", 1),
        ("notices.state", 1),
        ("notices.bounds", 1),
        ("notices.expired", 1),
        ("notices.next_expiry", 1),
    ]);
    let mut observed = BTreeSet::new();
    for (object, runs) in &costs.sql_by_object {
        assert_eq!(object, "fleet", "unexpected context alarm owner");
        for run in runs {
            let labels = run
                .iter()
                .map(|sql| {
                    *queries
                        .get(&normalize(sql))
                        .expect("unclassified alarm SQL changed the bounded owner inventory")
                })
                .collect::<Vec<_>>();
            assert!(labels.len() >= 9, "Fleet alarm omitted a maintenance owner");
            let (maintenance, mut dynamic) = labels.split_at(9);
            let mut counts = BTreeMap::new();
            for owner in maintenance {
                *counts.entry(*owner).or_insert(0_u64) += 1;
            }
            assert_eq!(
                counts, expected,
                "maintenance owner grew with retained data"
            );
            // The suffix is variable queued work. Recognize its SQL boundary,
            // but do not pretend this stream identifies individual dispatches:
            // Central's per-callback conformance tests own those cost bounds.

            while !dynamic.is_empty() {
                if let Some(rest) = dynamic.strip_prefix(&["directory.installation"]) {
                    dynamic = rest;
                } else {
                    dynamic = dynamic
                        .strip_prefix(&NOTICE_APPEND_QUERY_OWNERS)
                        .expect("variable work no longer matches the inventoried notice callback");
                }
            }
            let mut owners = BTreeMap::new();
            for (query, count) in counts {
                let owner = query
                    .split('.')
                    .next()
                    .expect("owner-qualified query identity");
                *owners.entry(owner).or_insert(0_u64) += count;
            }
            assert_eq!(
                owners,
                BTreeMap::from([("feed", 2), ("forwards", 2), ("notices", 5)])
            );
            observed.insert(owners);
        }
    }
    assert!(
        !observed.is_empty(),
        "no actual Fleet maintenance was observed"
    );
    observed
}

#[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
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
    #[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
    pub async fn finish_after_new_alarm(
        self,
    ) -> Result<AlarmRunCosts, Box<dyn std::error::Error + Send + Sync>> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let mut completed = completed_runs().subscribe();
        tokio::time::timeout_at(deadline, async {
            loop {
                completed.borrow_and_update();
                if state().lock().unwrap().next_id > self.first_new_run {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                completed
                    .changed()
                    .await
                    .map_err(|_| "alarm observation closed")?;
            }
        })
        .await
        .map_err(|_| "public probe did not start an actual Fleet alarm")??;
        self.finish_at(deadline).await
    }

    pub async fn finish(self) -> Result<AlarmRunCosts, Box<dyn std::error::Error + Send + Sync>> {
        self.finish_at(tokio::time::Instant::now() + Duration::from_secs(20))
            .await
    }

    #[expect(clippy::expect_used, reason = "Fail malformed fixed fixtures.")]
    #[expect(clippy::unwrap_used, reason = "Fail malformed fixed fixtures.")]
    async fn finish_at(
        self,
        deadline: tokio::time::Instant,
    ) -> Result<AlarmRunCosts, Box<dyn std::error::Error + Send + Sync>> {
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
        tokio::time::timeout_at(deadline, async {
            loop {
                completed.borrow_and_update();
                let completed_costs = {
                    let state = state().lock().unwrap();
                    let runs = selected
                        .iter()
                        .map(|id| state.runs.get(id).expect("observed alarm run"))
                        .collect::<Vec<_>>();
                    if runs.iter().all(|run| run.completed) {
                        let mut by_object: BTreeMap<String, Vec<u64>> = BTreeMap::new();
                        let mut sql_by_object: BTreeMap<String, Vec<Vec<String>>> = BTreeMap::new();
                        for run in runs {
                            by_object
                                .entry(run.object_kind.clone())
                                .or_default()
                                .push(run.statements);
                            sql_by_object
                                .entry(run.object_kind.clone())
                                .or_default()
                                .push(run.sql.clone());
                        }
                        Some((by_object, sql_by_object))
                    } else {
                        None
                    }
                };
                if let Some((by_object, sql_by_object)) = completed_costs {
                    return Ok(AlarmRunCosts {
                        overlapping_statements,
                        by_object,
                        sql_by_object,
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
