//! The SQLx Durable Object driver's scenarios, each inside a real SQLite-backed
//! Durable Object, behind the `test-support` feature. Moved with the driver from
//! agent-fleet-sync, where they ran as a separate Worker crate.
//!
//! `GET /test-support/driver/` lists the scenarios; `GET /test-support/driver/<scenario>`
//! runs one and answers `{"ok": true}` or `{"ok": false, "error": "..."}`. Every
//! request goes to one Durable Object, which runs the scenario against its own
//! storage. The assertions live here, in Rust next to the code they exercise, and
//! `verification/targets/workerd-central-storage.test.ts` only walks the list -- so
//! adding a scenario is one function and one line in `SCENARIOS`.

use std::future::Future;
use std::pin::Pin;

use sqlx_durable_object::DoConnection;
use worker::{DurableObject, Env, Request, Response, State, durable_object, wasm_bindgen};

type Outcome = Result<(), String>;
type Scenario = for<'c> fn(&'c mut DoConnection) -> Pin<Box<dyn Future<Output = Outcome> + 'c>>;

macro_rules! scenarios {
    ($($name:ident),* $(,)?) => {
        const SCENARIOS: &[(&str, Scenario)] = &[
            $((stringify!($name), |conn| Box::pin($name(conn)))),*
        ];
    };
}

scenarios![
    select_literals,
    bind_round_trip,
    null_values,
    whole_real_decodes_as_f64,
    blob_round_trip,
    integer_bounds,
    duplicate_column_names,
    empty_result,
    fetch_optional_takes_the_first_row,
    execute_reports_changes,
    constraint_errors,
    batch_reports_each_statement,
    batch_is_atomic,
    begin_fails,
    from_row,
    invalid_sql_is_an_error,
    wrong_argument_count_is_an_error,
    empty_batch,
    batch_with_unencodable_argument_runs_nothing,
    text_edge_cases,
    blob_edge_cases,
    concurrent_queries_share_a_connection,
    query_builder_bulk_insert,
    derived_types,
    fetch_batch_returns_rows_and_results,
    fetch_batch_serves_sqlc_batch_kinds,
    fetch_batch_is_atomic,
    fetch_batch_of_nothing_is_empty,
    fetch_batch_keeps_duplicate_column_names,
    transaction_commits_and_returns_its_value,
    transaction_rolls_back_when_the_callback_fails,
    transaction_keeps_the_statement_error,
    transaction_reads_its_own_writes,
    select_reports_no_changes,
    begin_statement_is_refused,
];

const SCHEMA: &str = include_str!("driver_scenarios/schema.sql");

// Every scenario in one object: they share its storage, as the D1 Worker's
// share one database, and `reset` clears what each one depends on.
// Scenarios the Worker runs itself rather than the object: they need more
// than one request to the object at a time.
const WORKER_SCENARIOS: &[&str] = &["transaction_holds_off_other_requests"];

/// The binding of the scenario object class in the test-support bundle.
pub(crate) const BINDING: &str = "DRIVER_SCENARIOS";

/// Answers `scenario`, the path after `/test-support/driver/`; empty lists them.
pub(crate) async fn route(scenario: &str, env: &Env) -> worker::Result<Response> {
    let stub = env
        .durable_object(BINDING)?
        .id_from_name("scenarios")?
        .get_stub()?;

    match scenario {
        "" => {
            let mut names: Vec<String> = stub.fetch_with_str("https://do/").await?.json().await?;
            names.extend(WORKER_SCENARIOS.iter().map(|name| (*name).to_owned()));
            Response::from_json(&names)
        }
        "transaction_holds_off_other_requests" => {
            let body = match transaction_holds_off_other_requests(env).await {
                Ok(()) => serde_json::json!({ "ok": true }),
                Err(error) => serde_json::json!({ "ok": false, "error": error }),
            };
            Response::from_json(&body)
        }
        _ => stub.fetch_with_str(&format!("https://do/{scenario}")).await,
    }
}

#[durable_object]
pub struct DriverScenarioObject {
    state: State,
}

impl DurableObject for DriverScenarioObject {
    fn new(state: State, _env: Env) -> Self {
        Self { state }
    }

    async fn fetch(&self, req: Request) -> worker::Result<Response> {
        let name = req.path().trim_start_matches('/').to_owned();

        if name.is_empty() {
            let names: Vec<_> = SCENARIOS.iter().map(|(name, _)| *name).collect();
            return Response::from_json(&names);
        }

        // The steps of the Worker-level scenarios, which are not scenarios
        // themselves.
        if let Some(op) = name.strip_prefix("op/") {
            let conn = DoConnection::new(self.state.storage());
            conn.sql().exec(SCHEMA, None)?;
            return op_step(&conn, op).await;
        }

        let Some((_, scenario)) = SCENARIOS.iter().find(|(n, _)| *n == name) else {
            return Response::error(format!("no scenario `{name}`"), 404);
        };

        // A fresh connection per request, from the object's own storage:
        // `begin_fails` needs one it can borrow mutably.
        let mut conn = DoConnection::new(self.state.storage());
        conn.sql().exec(SCHEMA, None)?;

        let body = match scenario(&mut conn).await {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };

        Response::from_json(&body)
    }
}

/// `Err` with the message unless `condition` holds.
fn ensure(condition: bool, message: impl Into<String>) -> Outcome {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn fail(error: sqlx::Error) -> String {
    error.to_string()
}

async fn reset(conn: &DoConnection) -> Outcome {
    for table in ["posts", "users", "numbers"] {
        sqlx::query(sqlx::AssertSqlSafe(format!("DELETE FROM {table}")))
            .execute(conn)
            .await
            .map_err(fail)?;
    }
    Ok(())
}

async fn insert_user(conn: &DoConnection, email: &str) -> Result<i64, String> {
    sqlx::query_scalar("INSERT INTO users (email, name) VALUES (?, ?) RETURNING id")
        .bind(email)
        .bind("someone")
        .fetch_one(conn)
        .await
        .map_err(fail)
}

#[path = "driver_scenarios/batch_scenarios.rs"]
mod batch_scenarios;
#[path = "driver_scenarios/concurrent_requests.rs"]
mod concurrent_requests;
#[path = "driver_scenarios/row_scenarios.rs"]
mod row_scenarios;
#[path = "driver_scenarios/transaction_scenarios.rs"]
mod transaction_scenarios;
use batch_scenarios::{
    fetch_batch_is_atomic, fetch_batch_keeps_duplicate_column_names,
    fetch_batch_of_nothing_is_empty, fetch_batch_returns_rows_and_results,
    fetch_batch_serves_sqlc_batch_kinds,
};
use concurrent_requests::{op_step, transaction_holds_off_other_requests};
use row_scenarios::{
    batch_is_atomic, batch_reports_each_statement, batch_with_unencodable_argument_runs_nothing,
    begin_fails, begin_statement_is_refused, bind_round_trip, blob_edge_cases, blob_round_trip,
    concurrent_queries_share_a_connection, constraint_errors, derived_types,
    duplicate_column_names, empty_batch, empty_result, execute_reports_changes,
    fetch_optional_takes_the_first_row, from_row, integer_bounds, invalid_sql_is_an_error,
    null_values, query_builder_bulk_insert, select_literals, select_reports_no_changes,
    text_edge_cases, whole_real_decodes_as_f64, wrong_argument_count_is_an_error,
};
use transaction_scenarios::{
    transaction_commits_and_returns_its_value, transaction_keeps_the_statement_error,
    transaction_reads_its_own_writes, transaction_rolls_back_when_the_callback_fails,
};

#[derive(Debug, sqlx::FromRow)]
struct User {
    id: i64,
    email: String,
    name: Option<String>,
    age: Option<i32>,
    score: Option<f64>,
    active: bool,
    avatar: Option<Vec<u8>>,
}
