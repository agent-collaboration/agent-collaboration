//! A SQLite Durable Object that runs the storage boundary's O1 cases on the real
//! statement store, plus the pause seam and a simulated crash. Test-support only.

use super::case_steps::{CaseBatch, CaseMigrator, CasePausePoint, CaseRead};
use crate::statement_store::{self, AlarmPolicy};
use crate::store_wire::OwnedStatement;
use central_storage::{AlarmUpdate, RowExpectation, SafeInteger, SqlValue, StatementBatch};
use collaboration_central_core::{
    ClockIds, HeldPause, PausePoint, PausePoints, WorkspaceTurns, storage_conformance,
};
use message_board::WorkspaceId;
use runtime_primitives::{Clock, IdSource, SyncTime};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx_durable_object::DoConnection;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::Arc;
use worker::{DurableObject, Env, Request, Response, Result, State, durable_object, wasm_bindgen};

/// A clock a case holds still, so ids minted in one batch share one millisecond.
struct HeldClock(SyncTime);
impl Clock for HeldClock {
    fn now(&self) -> SyncTime {
        self.0
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MintOrdered {
    count: usize,
    held_at_ms: i64,
    owner_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PauseTarget {
    workspace: WorkspaceId,
    point: CasePausePoint,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WaiterTarget {
    workspace: WorkspaceId,
    count: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeldRequest {
    workspace: WorkspaceId,
    batch: CaseBatch,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlarmRollback {
    at_ms: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SeamState {
    held: Vec<CasePausePoint>,
    waiting: usize,
}

#[durable_object]
pub struct StorageCaseObject {
    state: State,
    connection: DoConnection,
    opened: Cell<bool>,
    pauses: PausePoints,
    turns: Arc<WorkspaceTurns>,
    holds: RefCell<BTreeMap<(WorkspaceId, CasePausePoint), HeldPause>>,
}

/// A case transaction's own failure, so it can roll storage back.
struct CaseFailure;
impl From<sqlx_core::Error> for CaseFailure {
    fn from(_: sqlx_core::Error) -> Self {
        Self
    }
}

fn case_error(error: impl std::fmt::Display) -> worker::Error {
    worker::Error::RustError(error.to_string())
}

impl StorageCaseObject {
    async fn open(&self, migrator: CaseMigrator) -> std::result::Result<(), String> {
        if self.opened.get() {
            return Ok(());
        }
        statement_store::migrate(&self.connection, Arc::new(migrator.migrator()))
            .await
            .map_err(|failure| failure.to_string())?;
        self.opened.set(true);
        Ok(())
    }

    fn seam_state(&self, workspace: &WorkspaceId) -> SeamState {
        SeamState {
            held: self
                .pauses
                .held(workspace)
                .into_iter()
                .map(CasePausePoint::from)
                .collect(),
            waiting: self.turns.waiting(workspace),
        }
    }

    async fn run(&self, batch: CaseBatch) -> Result<Response> {
        let outcome = statement_store::run_batch(
            &self.connection,
            batch.into_batch().into(),
            AlarmPolicy::Apply(self.state.storage()),
        )
        .await;
        Response::from_json(&outcome)
    }

    /// A request on the central path: take the workspace's turn, read, apply, then
    /// pass the later points. Each point holds only when a case armed it.
    async fn held_request(&self, request: HeldRequest) -> Result<Response> {
        let turn = self.turns.take(&request.workspace).await;
        let read = statement_store::read(
            &self.connection,
            OwnedStatement::from(storage_conformance::count_owners()),
        )
        .await;
        self.pauses
            .reached(&request.workspace, PausePoint::AfterReads)
            .await;
        let outcome = statement_store::run_batch(
            &self.connection,
            request.batch.into_batch().into(),
            AlarmPolicy::Ignore,
        )
        .await;
        self.pauses
            .reached(&request.workspace, PausePoint::AfterStoreApplied)
            .await;
        self.pauses
            .reached(&request.workspace, PausePoint::AfterNoticesCommitted)
            .await;
        drop(turn);
        Response::from_json(&json!({ "read": read, "outcome": outcome }))
    }

    async fn mint_ordered(&self, mint: MintOrdered) -> Result<Response> {
        let ids = ClockIds::new(HeldClock(SyncTime::from_unix_millis(mint.held_at_ms)));
        let minted: Vec<String> = (0..mint.count)
            .map(|_| ids.new_id().hyphenated().to_string())
            .collect();
        let mut statements = vec![storage_conformance::insert_owner(
            &mint.owner_id,
            RowExpectation::Exactly(1),
        )];
        let active = SqlValue::Integer(SafeInteger::new(1).map_err(case_error)?);
        statements.extend(minted.iter().enumerate().map(|(position, id)| {
            storage_conformance::insert_widget(
                id,
                &mint.owner_id,
                SqlValue::Text(format!("widget {position}")),
                active.clone(),
                None,
            )
        }));
        let outcome = statement_store::run_batch(
            &self.connection,
            StatementBatch {
                statements,
                alarm: AlarmUpdate::Unchanged,
            }
            .into(),
            AlarmPolicy::Ignore,
        )
        .await;
        Response::from_json(&json!({ "minted": minted, "outcome": outcome }))
    }

    /// Proves the platform property the alarm path relies on: an alarm written inside
    /// a storage transaction that then fails is rolled back with it.
    async fn alarm_rollback(&self, rollback: AlarmRollback) -> Result<Response> {
        let at = chrono::DateTime::from_timestamp_millis(rollback.at_ms)
            .ok_or_else(|| case_error("alarm time out of range"))?;
        let storage = self.state.storage();
        let failed = self
            .connection
            .transaction(move |_| async move {
                storage.set_alarm(at).await.map_err(|_| CaseFailure)?;
                // The case fails after setting the alarm.
                Err::<(), CaseFailure>(CaseFailure)
            })
            .await;
        Response::from_json(&json!({
            "transactionFailed": failed.is_err(),
            "alarmMs": self.state.storage().get_alarm().await?,
        }))
    }

    async fn dispatch(&self, mut request: Request, migrator: CaseMigrator) -> Result<Response> {
        let path = request.path();
        let operation = path.trim_start_matches('/');
        if operation == "open" {
            return Response::from_json(&match self.open(migrator).await {
                Ok(()) => json!({ "opened": true }),
                Err(error) => json!({ "opened": false, "error": error }),
            });
        }
        if let Err(error) = self.open(migrator).await {
            return Response::error(format!("the case object did not open: {error}"), 409);
        }
        match operation {
            "batch" => match serde_json::from_slice::<CaseBatch>(&request.bytes().await?) {
                Ok(batch) => self.run(batch).await,
                // The wire refuses a value it cannot carry, such as an integer
                // outside ±(2^53−1), before anything reaches storage.
                Err(refusal) => Ok(Response::from_json(
                    &json!({ "refused": refusal.to_string() }),
                )?
                .with_status(422)),
            },
            "read" => {
                let read: CaseRead = request.json().await?;
                Response::from_json(
                    &statement_store::read(&self.connection, read.statement().into()).await,
                )
            }
            "alarm" => {
                Response::from_json(&json!({ "alarmMs": self.state.storage().get_alarm().await? }))
            }
            "alarm-rollback" => self.alarm_rollback(request.json().await?).await,
            "mint-ordered" => self.mint_ordered(request.json().await?).await,
            "request" => self.held_request(request.json().await?).await,
            "hold" => {
                let target: PauseTarget = request.json().await?;
                let held = self.pauses.hold(&target.workspace, target.point.into());
                self.holds
                    .borrow_mut()
                    .insert((target.workspace, target.point), held);
                Response::from_json(&json!({ "armed": true }))
            }
            "until-held" => {
                let target: PauseTarget = request.json().await?;
                let key = (target.workspace.clone(), target.point);
                let mut held = self
                    .holds
                    .borrow_mut()
                    .remove(&key)
                    .ok_or_else(|| case_error("that point is not armed"))?;
                held.until_held().await;
                self.holds.borrow_mut().insert(key, held);
                Response::from_json(&self.seam_state(&target.workspace))
            }
            "until-waiting" => {
                let target: WaiterTarget = request.json().await?;
                self.turns
                    .until_waiting(&target.workspace, target.count)
                    .await;
                Response::from_json(&self.seam_state(&target.workspace))
            }
            "release" => {
                let target: PauseTarget = request.json().await?;
                let held = self
                    .holds
                    .borrow_mut()
                    .remove(&(target.workspace.clone(), target.point));
                if let Some(held) = held {
                    held.release();
                }
                Response::from_json(&self.seam_state(&target.workspace))
            }
            "crash" => {
                // Rejecting inside blockConcurrencyWhile resets the object: in-memory
                // state and every held request go, storage stays.
                self.state
                    .block_concurrency_while(async {
                        Err::<(), worker::Error>(case_error("simulated crash"))
                    })
                    .await?;
                Response::error("the object was not reset", 500)
            }
            _ => Response::error("unknown case operation", 404),
        }
    }
}

impl DurableObject for StorageCaseObject {
    fn new(state: State, _env: Env) -> Self {
        let connection = DoConnection::new(state.storage());
        Self {
            state,
            connection,
            opened: Cell::new(false),
            pauses: PausePoints::default(),
            turns: Arc::new(WorkspaceTurns::default()),
            holds: RefCell::new(BTreeMap::new()),
        }
    }

    async fn fetch(&self, request: Request) -> Result<Response> {
        let migrator: CaseMigrator = serde_json::from_str(
            &request
                .headers()
                .get("x-case-migrator")?
                .ok_or_else(|| case_error("x-case-migrator missing"))?,
        )?;
        self.dispatch(request, migrator).await
    }
}
