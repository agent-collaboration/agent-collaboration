//! Capture each real alarm's SQL, retaining unknown and mixed-owner statements.
use super::composed_fleet_fixture::TestResult;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};
use tracing::{
    Event, Subscriber,
    field::{Field, Visit},
    instrument::WithSubscriber,
};
use tracing_subscriber::{Layer, layer::Context, prelude::*};
use turso_parser::{
    ast::{Cmd, Stmt},
    parser::Parser,
};

#[derive(Clone, Default)]
pub(super) struct AlarmSql(Arc<Mutex<Vec<String>>>);

#[derive(Default)]
struct SqlStatement {
    statement: String,
    summary: String,
}
impl Visit for SqlStatement {
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "db.statement" => self.statement = value.to_owned(),
            "summary" => self.summary = value.to_owned(),
            _ => {}
        }
    }
    fn record_debug(&mut self, _: &Field, _: &dyn std::fmt::Debug) {}
}
impl<Trace: Subscriber> Layer<Trace> for AlarmSql {
    #[expect(
        clippy::expect_used,
        reason = "A poisoned test recorder must fail, never silently omit real SQL."
    )]
    fn on_event(&self, event: &Event<'_>, _: Context<'_, Trace>) {
        if event.metadata().target() != "sqlx::query" {
            return;
        }
        let mut sql = SqlStatement::default();
        event.record(&mut sql);
        self.0
            .lock()
            .expect("alarm SQL recorder")
            .push(if sql.statement.is_empty() {
                sql.summary
            } else {
                sql.statement
            });
    }
}
impl AlarmSql {
    pub(super) async fn observe<Callback: std::future::Future>(
        &self,
        callback: Callback,
    ) -> Callback::Output {
        callback
            .with_subscriber(tracing_subscriber::registry().with(self.clone()))
            .await
    }
    pub(super) fn statements(&self) -> TestResult<Vec<String>> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "alarm SQL recorder poisoned")?
            .clone())
    }
}

// The established Router SQL proof uses the same parser's table identities.
// An ambiguous parse, transaction command, unknown table, or mixed-owner query
// stays counted. Only statements exclusively touching SDK tables are excluded.
fn only_sdk_tables(sql: &str) -> Option<bool> {
    fn collect(node: &serde_json::Value, tables: &mut BTreeSet<String>) {
        match node {
            serde_json::Value::Object(fields) => {
                if fields.len() == 3
                    && fields.contains_key("db_name")
                    && fields.contains_key("alias")
                    && let Ok(name) =
                        serde_json::from_value::<turso_parser::ast::QualifiedName>(node.clone())
                {
                    tables.insert(name.name.as_str().to_ascii_lowercase());
                }
                for value in fields.values() {
                    collect(value, tables);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    collect(value, tables);
                }
            }
            _ => {}
        }
    }
    let mut parser = Parser::new(sql.as_bytes());
    let mut tables = BTreeSet::new();
    while let Some(command) = parser.next_cmd().ok()? {
        let (Cmd::Stmt(statement)
        | Cmd::Explain(statement)
        | Cmd::ExplainQueryPlan {
            stmt: statement, ..
        }) = command;
        if !matches!(
            statement,
            Stmt::Select(_) | Stmt::Insert { .. } | Stmt::Update(_) | Stmt::Delete { .. }
        ) {
            return None;
        }
        collect(&serde_json::to_value(statement).ok()?, &mut tables);
    }
    Some(!tables.is_empty() && tables.iter().all(|table| table.starts_with("_fleet_sync_")))
}

fn normalized(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The fixture has one established control stream, no expired data, and no
/// other task kinds. Actual dispatches and committed appends supply the counts.
pub(super) fn validate_alarm(sql: &[String], dispatches: usize, appends: usize) -> TestResult {
    const STATE: &str = include_str!(
        "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_state.sql"
    );
    const BOUNDS: &str = include_str!(
        "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_bounds.sql"
    );
    const EXPIRED: &str = include_str!(
        "../../../collaboration-central-core/sql/machine_notices/read_expired_machine_notices.sql"
    );
    const EXPIRY: &str = include_str!(
        "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_expiry.sql"
    );
    const LOOKUP: &str =
        include_str!("../../../collaboration-central-core/sql/read_enrollments.sql");
    let maintenance = [
        include_str!("../../../collaboration-central-core/sql/fleet/read_fleet_feed_state.sql"),
        include_str!("../../../collaboration-central-core/sql/fleet/read_expired_fleet_feed.sql"),
        include_str!("../../../collaboration-central-core/sql/forwards/read_forward_rows.sql"),
        include_str!("../../../collaboration-central-core/sql/forwards/read_forward_rows.sql"),
        include_str!(
            "../../../collaboration-central-core/sql/machine_notices/read_machine_notice_peers.sql"
        ),
        STATE,
        BOUNDS,
        EXPIRED,
        EXPIRY,
    ]
    .map(normalized);
    let owned: Vec<_> = sql
        .iter()
        .filter(|statement| only_sdk_tables(statement) != Some(true))
        .map(|statement| normalized(statement))
        .collect();
    let Some(variable) = owned.strip_prefix(&maintenance) else {
        return Err(format!(
            "Fleet alarm maintenance boundary changed: {} owned statements",
            owned.len()
        )
        .into());
    };
    let append = [STATE, BOUNDS, EXPIRED, STATE, BOUNDS,
        include_str!("../../../collaboration-central-core/sql/machine_notices/append_machine_notice.sql"),
        include_str!("../../../collaboration-central-core/sql/machine_notices/advance_machine_notice_head.sql"), EXPIRY,
    ].map(normalized);
    let mut remaining = variable;
    let mut observed = (0, 0);
    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix(&[normalized(LOOKUP)]) {
            observed.0 += 1;
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix(&append) {
            observed.1 += 1;
            remaining = rest;
        } else {
            return Err("variable Fleet task SQL changed its owning callback inventory".into());
        }
    }
    if observed != (dispatches, appends) || variable.len() != dispatches + 8 * appends {
        return Err(format!(
            "variable Fleet SQL counts {observed:?} differ from actual dispatch/append counts {:?}",
            (dispatches, appends)
        )
        .into());
    }
    Ok(())
}

#[test]
fn mixed_or_unknown_sql_cannot_disappear_into_sdk_bookkeeping() {
    assert_eq!(
        only_sdk_tables("SELECT id FROM _fleet_sync_operations"),
        Some(true)
    );
    assert_eq!(
        only_sdk_tables(
            "SELECT o.id FROM _fleet_sync_operations o JOIN machine_enrollments m ON m.machine_id=o.id"
        ),
        Some(false)
    );
    assert_eq!(
        only_sdk_tables("SELECT * FROM future_central_table"),
        Some(false)
    );
    assert_ne!(only_sdk_tables("malformed SQL"), Some(true));
}
