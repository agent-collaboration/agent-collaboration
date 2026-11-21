//! Controls the actual modern CentralObject only in test-support bundles.
use super::{
    AlarmFailure, AlarmPolicy, CentralObject, ObjectAddress, OrganizationId, statement_store,
};
use crate::store_wire::{OwnedBatch, OwnedStatement, OwnedStoreCall};
use fleet_runtime::{HeldPause, PausePoint};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};
use worker::{Env, Request, Response, Result};
use worker::{Headers, Method, RequestInit, js_sys};
#[path = "workspace_fixture_admission.rs"]
mod fixture_admission;
use crate::workspace_rpc_fixture;

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) faults: std::sync::Arc<fleet_runtime::TestFaults>,
    pub(super) zero_next_write_alarm: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) lose_read: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(super) legacy_workspace_calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    observations: RefCell<BTreeMap<workspace_ids::WorkspaceId, HeldPause>>,
    holds: RefCell<BTreeMap<(workspace_ids::WorkspaceId, Point), HeldPause>>,
    central_holds: RefCell<BTreeMap<Point, HeldPause>>,
    engine_holds: RefCell<BTreeMap<u64, HeldPause>>,
    next_engine_hold: std::cell::Cell<u64>,
    /// A probe caller admitted once, ahead of a held turn, so a request can
    /// run without its own admission.
    admitted_probe_caller: RefCell<Option<collaboration_central_core::WorkspaceCaller>>,
    alarm_waiters: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
}
#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
enum Point {
    AfterReads,
    AfterStoreApplied,
    AfterWorkspaceExecute,
    EngineCompletionEntered,
    BeforeDrainCommit,
    AfterDrainCommit,
    BeforeTimerCommit,
    AfterTimerCommit,
    AfterEngineAttemptGate,
    AfterEngineLoad,
    AfterEngineCommit,
    BeforeEngineFailureWrite,
    AfterEngineAbsentLookup,
    BeforeControlClear,
    BeforeSignInSample,
    AfterSignInSample,
}
impl From<Point> for PausePoint {
    fn from(point: Point) -> Self {
        match point {
            Point::AfterReads => Self::AfterReads,
            Point::AfterStoreApplied => Self::AfterStoreApplied,
            Point::AfterWorkspaceExecute => Self::AfterWorkspaceExecute,
            Point::EngineCompletionEntered => Self::EngineCompletionEntered,
            Point::BeforeDrainCommit => Self::BeforeDrainCommit,
            Point::AfterDrainCommit => Self::AfterDrainCommit,
            Point::BeforeTimerCommit => Self::BeforeTimerCommit,
            Point::AfterTimerCommit => Self::AfterTimerCommit,
            Point::AfterEngineAttemptGate => Self::AfterEngineAttemptGate,
            Point::AfterEngineLoad => Self::AfterEngineLoad,
            Point::AfterEngineCommit => Self::AfterEngineCommit,
            Point::BeforeEngineFailureWrite => Self::BeforeEngineFailureWrite,
            Point::AfterEngineAbsentLookup => Self::AfterEngineAbsentLookup,
            Point::BeforeControlClear => Self::BeforeControlClear,
            Point::BeforeSignInSample => Self::BeforeSignInSample,
            Point::AfterSignInSample => Self::AfterSignInSample,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EnginePause {
    point: Point,
    key: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pause {
    workspace: workspace_ids::WorkspaceId,
    point: Point,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wait {
    workspace: workspace_ids::WorkspaceId,
    count: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    workspace: workspace_ids::WorkspaceId,
    call: OwnedStoreCall,
}
fn error(message: impl ToString) -> worker::Error {
    worker::Error::RustError(message.to_string())
}

pub(crate) async fn route(
    mut request: Request,
    env: &Env,
) -> Result<std::result::Result<Response, Request>> {
    let path = request.path();
    let Some(rest) = path.strip_prefix("/test-support/workspace/") else {
        return Ok(Err(request));
    };
    let Some((organization, operation)) = rest.split_once('/') else {
        return Ok(Ok(Response::error("missing operation", 400)?));
    };
    let organization: OrganizationId =
        serde_json::from_value(serde_json::Value::String(organization.to_owned()))?;
    let address = ObjectAddress::Central(organization);
    let headers = Headers::new();
    crate::cloudflare_transport::address_header(&headers, &address)?;
    let body = request.bytes().await?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
    let path = format!("/test/{operation}");
    let forwarded = Request::new_with_init(&format!("https://central.internal{path}"), &init)?;
    Ok(Ok(crate::cloudflare_transport::stub(env, &address)?
        .fetch_with_request(forwarded)
        .await?))
}
pub(super) async fn control(
    object: &CentralObject,
    organization: OrganizationId,
    mut request: Request,
) -> Result<Response> {
    let path = request.path();
    if path == "/test/machine-login-revoke" {
        let control: collaboration_central_core::MachineLoginControl = request.json().await?;
        let runtime = object.runtime(organization).await?;
        return Response::from_json(
            &collaboration_central_core::machine_login_control(runtime.as_ref(), control).await,
        );
    }
    if matches!(path.as_str(), "/test/request" | "/test/mail") {
        return fixture_admission::dispatch(object, organization, request).await;
    }
    // Holding time before initialization also pins the first minted identity.
    if path == "/test/clock" {
        object.clock.hold(request.json::<i64>().await?);
        return Response::from_json(&true);
    }
    let runtime = object.runtime(organization.clone()).await?;
    match path.as_str() {
        "/test/completion-attempts" => {
            Response::from_json(&runtime.runtime().completion_attempts())
        }
        "/test/engine-completion-entries" => {
            Response::from_json(&runtime.runtime().engine_completion_entries())
        }
        "/test/try-completion-turns" => {
            let workspace = request.json::<workspace_ids::WorkspaceId>().await?;
            let central_free = runtime.runtime().try_central_turn().is_some();
            let workspace_free = runtime
                .runtime()
                .workspace_turns()
                .try_take(&workspace)
                .is_some();
            Response::from_json(&[central_free, workspace_free])
        }
        "/test/zero-next-write-alarm" => {
            object
                .controls
                .zero_next_write_alarm
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Response::from_json(&true)
        }
        "/test/legacy-unit-invocations" => {
            Response::from_json(&runtime.runtime().legacy_unit_invocations())
        }
        "/test/due-unit-starts" => Response::from_json(&runtime.runtime().due_unit_starts()),
        "/test/engine-commit-fault-pending" => {
            Response::from_json(&object.controls.faults.commit_pending())
        }
        "/test/engine-commit-fault" => {
            let fault = request.json().await?;
            object.controls.faults.arm_commit(fault);
            Response::from_json(&true)
        }
        "/test/engine-read-fault" => {
            let fault = request.json().await?;
            object.controls.faults.arm_read(fault);
            Response::from_json(&true)
        }
        "/test/machine-restore-failure" => {
            let (skip, failure) = request.json().await?;
            runtime.fail_machine_restore_after(skip, failure);
            Response::from_json(&true)
        }
        "/test/engine-probe-seed" => {
            Response::from_json(&runtime.seed_probe(request.json().await?).await)
        }
        "/test/engine-probe-request" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct ProbeCall {
                caller: workspace_model::Caller,
                token: String,
                request: collaboration_central_core::engine_probe::ProbeRequest,
            }
            let call: ProbeCall = request.json().await?;
            let Some(caller) =
                fixture_admission::admitted_caller(&runtime, &call.token, call.caller).await?
            else {
                return Response::error("probe caller not admitted", 401);
            };
            Response::from_json(&runtime.request_probe(&caller, call.request).await)
        }
        "/test/engine-owed-seed" => {
            Response::from_json(&runtime.seed_owed_reply(request.json().await?).await)
        }
        "/test/engine-owed-request" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct OwedCall {
                caller: workspace_model::Caller,
                token: String,
                request: collaboration_central_core::engine_owed_probe::OwedReplyRequest,
            }
            let call: OwedCall = request.json().await?;
            let Some(caller) =
                fixture_admission::admitted_caller(&runtime, &call.token, call.caller).await?
            else {
                return Response::error("probe caller not admitted", 401);
            };
            Response::from_json(&runtime.request_owed_reply(&caller, call.request).await)
        }
        "/test/lose-provisioning-reply" => {
            runtime.lose_next_provisioning_reply();
            Response::from_json(&true)
        }
        "/test/fail-alarm-batch" => {
            let failure = request.json().await?;
            object.controls.faults.arm_alarm_batch(failure);
            Response::from_json(&true)
        }
        "/test/drain" => {
            let workspace = request.json().await?;
            Response::from_json(&runtime.drain_workspace(&workspace).await)
        }
        "/test/queue" => {
            let session: workspace_ids::SessionRef = request.json().await?;
            Response::from_json(&runtime.queued_for_session(&session).await)
        }
        "/test/alarm" => Response::from_json(&object.state.storage().get_alarm().await?),
        "/test/hold-central" => {
            let point: Point = request.json().await?;
            if !matches!(
                point,
                Point::BeforeTimerCommit
                    | Point::AfterTimerCommit
                    | Point::AfterEngineLoad
                    | Point::BeforeSignInSample
                    | Point::AfterSignInSample
                    | Point::AfterWorkspaceExecute
                    | Point::EngineCompletionEntered
            ) {
                return Err(error("not a timer point"));
            }
            object.controls.central_holds.borrow_mut().insert(
                point,
                runtime.runtime().pause_points().hold_central(point.into()),
            );
            Response::from_json(&true)
        }
        "/test/hold-engine" => {
            let pause: EnginePause = request.json().await?;
            let id = object.controls.next_engine_hold.get();
            object.controls.next_engine_hold.set(id + 1);
            let held = runtime
                .runtime()
                .pause_points()
                .hold_engine(pause.point.into(), &pause.key);
            object.controls.engine_holds.borrow_mut().insert(id, held);
            Response::from_json(&id)
        }
        "/test/until-held-engine" => {
            let id: u64 = request.json().await?;
            let mut held = object
                .controls
                .engine_holds
                .borrow_mut()
                .remove(&id)
                .ok_or_else(|| error("engine hold not armed"))?;
            held.until_held().await;
            object.controls.engine_holds.borrow_mut().insert(id, held);
            Response::from_json(&true)
        }
        "/test/release-engine" => {
            let id: u64 = request.json().await?;
            if let Some(held) = object.controls.engine_holds.borrow_mut().remove(&id) {
                held.release();
            }
            Response::from_json(&true)
        }
        "/test/until-central-waiting" => {
            let count: usize = request.json().await?;
            runtime.runtime().until_central_waiting(count).await;
            Response::from_json(&true)
        }
        "/test/until-admission-waiting" => {
            let count: usize = request.json().await?;
            runtime.runtime().until_admission_waiting(count).await;
            Response::from_json(&true)
        }
        "/test/admission-waiting" => {
            Response::from_json(&runtime.runtime().admission_waiting_now())
        }
        "/test/engine-probe-admit" => {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase", deny_unknown_fields)]
            struct AdmitCall {
                caller: workspace_model::Caller,
                token: String,
            }
            let call: AdmitCall = request.json().await?;
            let admitted =
                fixture_admission::admitted_caller(&runtime, &call.token, call.caller).await?;
            let accepted = admitted.is_some();
            *object.controls.admitted_probe_caller.borrow_mut() = admitted;
            Response::from_json(&accepted)
        }
        "/test/engine-probe-request-admitted" => {
            let probe: collaboration_central_core::engine_probe::ProbeRequest =
                request.json().await?;
            let admitted = object.controls.admitted_probe_caller.borrow().clone();
            let Some(caller) = admitted else {
                return Response::error("no admitted probe caller", 409);
            };
            Response::from_json(&runtime.request_probe(&caller, probe).await)
        }
        "/test/until-held-central" => {
            let point: Point = request.json().await?;
            let mut held = object
                .controls
                .central_holds
                .borrow_mut()
                .remove(&point)
                .ok_or_else(|| error("point not armed"))?;
            held.until_held().await;
            object
                .controls
                .central_holds
                .borrow_mut()
                .insert(point, held);
            Response::from_json(&true)
        }
        "/test/release-central" => {
            let point: Point = request.json().await?;
            if let Some(held) = object.controls.central_holds.borrow_mut().remove(&point) {
                held.release();
            }
            Response::from_json(&true)
        }
        "/test/central-batch" => {
            let batch = request.json::<OwnedBatch>().await?;
            let policy = match &batch.alarm {
                crate::store_wire::OwnedAlarmUpdate::SetEngineFromQuery { .. } => {
                    AlarmPolicy::Engine(
                        object.state.storage(),
                        object.clock.clone(),
                        fleet_runtime::engine_alarm_decider(object.due_batches.clone()),
                    )
                }
                crate::store_wire::OwnedAlarmUpdate::Unchanged
                | crate::store_wire::OwnedAlarmUpdate::SetFromQuery(_) => AlarmPolicy::Ignore,
            };
            Response::from_json(
                &statement_store::run_batch(&object.connection, batch, policy).await,
            )
        }
        "/test/central-read" => Response::from_json(
            &statement_store::read(&object.connection, request.json::<OwnedStatement>().await?)
                .await,
        ),
        "/test/store" => {
            let store: Store = request.json().await?;
            Response::from_json(
                &crate::cloudflare_transport::call_store(&object.env, store.workspace, store.call)
                    .await,
            )
        }
        "/test/store-failing-upgrade" => {
            let store: Store = request.json().await?;
            Response::from_json(
                &crate::cloudflare_transport::call_failing_workspace_store(
                    &object.env,
                    store.workspace,
                    store.call,
                )
                .await,
            )
        }
        "/test/reset-store" => {
            let workspace: workspace_ids::WorkspaceId = request.json().await?;
            let address = ObjectAddress::WorkspaceStore(workspace);
            let headers = Headers::new();
            crate::cloudflare_transport::address_header(&headers, &address)?;
            let mut init = RequestInit::new();
            init.with_method(Method::Post).with_headers(headers);
            let response = crate::cloudflare_transport::stub(&object.env, &address)?
                .fetch_with_request(Request::new_with_init(
                    "https://central.internal/test/crash",
                    &init,
                )?)
                .await;
            Response::from_json(
                &response
                    .map(|response| response.status_code() != 200)
                    .unwrap_or(true),
            )
        }
        "/test/central-failing-upgrade" => Response::from_json(
            &statement_store::migrate(
                &object.connection,
                std::sync::Arc::new(fleet_storage_schema::workspace_migrations::failing_upgrade(
                    fleet_storage_schema::workspace_migrations::central_migrator(),
                )),
            )
            .await
            .map_err(|error| error.to_string()),
        ),
        "/test/lose-read" => {
            object
                .controls
                .lose_read
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Response::from_json(&true)
        }
        "/test/lose-reply" => {
            runtime
                .runtime()
                .lose_reply_after_apply(request.json().await?);
            Response::from_json(&true)
        }
        "/test/hold" => {
            let target: Pause = request.json().await?;
            let held = runtime
                .runtime()
                .pause_points()
                .hold(&target.workspace, target.point.into());
            object
                .controls
                .holds
                .borrow_mut()
                .insert((target.workspace, target.point), held);
            Response::from_json(&true)
        }
        "/test/until-held" => {
            let target: Pause = request.json().await?;
            let key = (target.workspace, target.point);
            let mut held = object
                .controls
                .holds
                .borrow_mut()
                .remove(&key)
                .ok_or_else(|| error("point not armed"))?;
            held.until_held().await;
            object.controls.holds.borrow_mut().insert(key, held);
            Response::from_json(&true)
        }
        "/test/guard-next-read" => {
            let wait: Wait = request.json().await?;
            object.controls.observations.borrow_mut().insert(
                wait.workspace.clone(),
                runtime
                    .runtime()
                    .pause_points()
                    .hold(&wait.workspace, PausePoint::AfterReads),
            );
            Response::from_json(&true)
        }
        "/test/waiting-before-read" => {
            let wait: Wait = request.json().await?;
            let mut held = object
                .controls
                .observations
                .borrow_mut()
                .remove(&wait.workspace)
                .ok_or_else(|| error("read observation not armed"))?;
            let before_read = {
                let read = held.until_held();
                let waiting = runtime
                    .runtime()
                    .workspace_turns()
                    .until_waiting(&wait.workspace, wait.count);
                futures_util::pin_mut!(read, waiting);
                matches!(
                    futures_util::future::select(read, waiting).await,
                    futures_util::future::Either::Right(_)
                )
            };
            Response::from_json(&before_read)
        }
        "/test/crash" => {
            object
                .state
                .block_concurrency_while(async {
                    Err::<(), worker::Error>(error("workspace fixture crash"))
                })
                .await?;
            Response::error("object did not reset", 500)
        }
        "/test/until-waiting" => {
            let wait: Wait = request.json().await?;
            runtime
                .runtime()
                .workspace_turns()
                .until_waiting(&wait.workspace, wait.count)
                .await;
            Response::from_json(&true)
        }
        "/test/release" => {
            let target: Pause = request.json().await?;
            let held = object
                .controls
                .holds
                .borrow_mut()
                .remove(&(target.workspace, target.point));
            if let Some(held) = held {
                held.release();
            }
            Response::from_json(&true)
        }
        _ => Response::error("unknown workspace control", 404),
    }
}

const CLOCK_KEY: &str = "_workspace_test_clock_ms";
const RECEIPT_KEY: &str = "_workspace_test_alarm_receipt";
const ALARM_COUNT_KEY: &str = "_workspace_test_alarm_count";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AlarmReceipt {
    organization: Option<OrganizationId>,
    initialized_cold: bool,
    alarm_before: Option<i64>,
    alarm_after: Option<i64>,
    result: AlarmReceiptResult,
}

#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum AlarmReceiptResult {
    Applied {
        wakes_fired: usize,
        automations_fired: usize,
        windows_fired: usize,
        workspaces_drained: usize,
    },
    Error {
        message: String,
        failure: Option<central_storage::StoreFailure>,
    },
    Unprovisioned,
}

pub(super) async fn raw_control(object: &CentralObject, mut request: Request) -> Result<Response> {
    let storage = object.state.storage();
    match request.path().as_str() {
        "/test/raw-legacy-workspace-call-count" => Response::from_json(
            &object
                .controls
                .legacy_workspace_calls
                .load(std::sync::atomic::Ordering::SeqCst),
        ),
        "/test/raw-workspace-storage-state" => {
            let workspace = request.json().await?;
            let state =
                workspace_rpc_fixture::workspace_storage_state(&object.env, workspace).await?;
            Response::from_json(&state)
        }
        "/test/raw-workspace-rpc-probe" => {
            workspace_rpc_fixture::probe(&object.env, request.json().await?).await
        }
        "/test/raw-alarm-count" => {
            Response::from_json(&storage.get::<u64>(ALARM_COUNT_KEY).await?.unwrap_or(0))
        }
        "/test/raw-dispatch-due" => {
            let now: i64 = request.json().await?;
            // The platform scheduler uses its own time. Advance the test's one
            // injected clock and dispatch only its actually retained deadline.
            // A missing, later or cleared alarm must never fabricate a callback.
            let due = storage.get_alarm().await?;
            if due.is_none_or(|due| due > now) {
                return Response::from_json(&false);
            }
            storage.put(CLOCK_KEY, &now).await?;
            storage.delete(RECEIPT_KEY).await?;
            object.clock.hold(now);
            storage.set_alarm(0_i64).await?;
            Response::from_json(&true)
        }
        "/test/raw-alarm-status" => {
            Response::from_json(&storage.get::<AlarmReceipt>(RECEIPT_KEY).await?)
        }
        "/test/raw-until-alarm" => {
            let (sender, receiver) = futures_channel::oneshot::channel();
            object.controls.alarm_waiters.borrow_mut().push(sender);
            if let Some(receipt) = storage.get::<AlarmReceipt>(RECEIPT_KEY).await? {
                return Response::from_json(&receipt);
            }
            receiver
                .await
                .map_err(|_| error("alarm receipt wait interrupted"))?;
            let receipt = storage
                .get::<AlarmReceipt>(RECEIPT_KEY)
                .await?
                .ok_or_else(|| error("alarm receipt missing after notification"))?;
            Response::from_json(&receipt)
        }
        "/test/raw-alarm-time" => Response::from_json(&storage.get_alarm().await?),
        "/test/raw-drop-alarm" => {
            storage.delete_alarm().await?;
            Response::from_json(&true)
        }
        "/test/raw-probe-alarm" => {
            let milliseconds: i64 = request.json().await?;
            let prior = storage.get_alarm().await?;
            let result: std::result::Result<Option<i64>, String> =
                match chrono::DateTime::from_timestamp_millis(milliseconds) {
                    Some(at) => match storage.set_alarm(at).await {
                        Ok(()) => storage
                            .get_alarm()
                            .await
                            .map_err(|failure| failure.to_string()),
                        Err(failure) => Err(failure.to_string()),
                    },
                    None => Err("timestamp cannot be represented".to_owned()),
                };
            match prior {
                Some(previous) => {
                    let at = chrono::DateTime::from_timestamp_millis(previous)
                        .ok_or_else(|| error("previous alarm timestamp cannot be represented"))?;
                    storage.set_alarm(at).await?;
                }
                None => storage.delete_alarm().await?,
            }
            Response::from_json(&result)
        }
        "/test/raw-read" => {
            let statement: OwnedStatement = request.json().await?;
            if ![
                "SELECT (SELECT COUNT(*) FROM automation_wakes WHERE state='active'), (SELECT COUNT(*) FROM automation_wakes WHERE state='done'), (SELECT COUNT(*) FROM mail_messages WHERE message_kind='wake')",
                "SELECT id,organization_statement,created_at,updated_at FROM signin_organization_identity ORDER BY id",
                "SELECT * FROM signin_change_records ORDER BY id",
            ].contains(&statement.sql.as_str())
                || !statement.args.is_empty()
            {
                return Err(error(
                    "raw read only permits named corruption snapshots",
                ));
            }
            Response::from_json(&statement_store::read(&object.connection, statement).await)
        }
        "/test/raw-clear-alarm-receipt" => Response::from_json(&storage.delete(RECEIPT_KEY).await?),
        "/test/raw-set-stamps" => {
            let rows: Vec<(String, String)> = request.json().await?;
            if rows.len() > 2 {
                return Err(error("at most two fixture stamps"));
            }
            let mut statements = vec![central_storage::GuardedStatement {
                statement: central_storage::Statement {
                    sql: "DELETE FROM signin_organization_identity",
                    args: vec![],
                },
                expect: central_storage::RowExpectation::Any,
            }];
            for (id, statement) in rows {
                statements.push(central_storage::GuardedStatement {
                    statement: central_storage::Statement { sql: "INSERT INTO signin_organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,?,0,0)", args: vec![central_storage::SqlValue::Text(id), central_storage::SqlValue::Text(statement)] },
                    expect: central_storage::RowExpectation::Exactly(1),
                });
            }
            let batch = central_storage::StatementBatch {
                statements,
                alarm: central_storage::AlarmUpdate::Unchanged,
            };
            let result =
                statement_store::run_batch(&object.connection, batch.into(), AlarmPolicy::Ignore)
                    .await;
            Response::from_json(&result)
        }
        "/test/raw-arm-alarm" | "/test/raw-arm-cold-alarm" => {
            let cold = request.path() == "/test/raw-arm-cold-alarm";
            let now: i64 = request.json().await?;
            storage.put(CLOCK_KEY, &now).await?;
            storage.delete(RECEIPT_KEY).await?;
            object.clock.hold(now);
            storage.set_alarm(0_i64).await?;
            if cold {
                object
                    .state
                    .block_concurrency_while(async {
                        Err::<(), worker::Error>(error("cold alarm fixture reset"))
                    })
                    .await?;
                return Response::error("object did not reset", 500);
            }
            Response::from_json(&true)
        }
        _ => Response::error("unknown raw control", 404),
    }
}

pub(super) async fn alarm_with_receipt(object: &CentralObject) -> Result<Response> {
    let storage = object.state.storage();
    let count = storage.get::<u64>(ALARM_COUNT_KEY).await?.unwrap_or(0);
    storage
        .put(ALARM_COUNT_KEY, &count.saturating_add(1))
        .await?;
    let alarm_before = storage.get_alarm().await?;
    if let Some(now) = storage.get::<i64>(CLOCK_KEY).await? {
        object.clock.hold(now);
    }
    let initialized_cold = object.runtime.lock().await.is_none();
    let mut organization = None;
    let outcome = async {
        let Some((org, runtime)) = object.alarm_runtime().await? else {
            return Ok(None);
        };
        organization = Some(org);
        let result = runtime.after_event().await;
        object.flush_ready_mail(&runtime).await;
        result
            .map(Some)
            .map_err(|error| AlarmFailure::Store(error.failure))
    }
    .await;
    let alarm_after = storage.get_alarm().await?;
    let result = match &outcome {
        Ok(Some(result)) => AlarmReceiptResult::Applied {
            wakes_fired: result.work.wakes_fired,
            automations_fired: result.work.automations_fired,
            windows_fired: result.work.windows_fired,
            workspaces_drained: result.work.workspaces_drained,
        },
        Ok(None) => AlarmReceiptResult::Unprovisioned,
        Err(AlarmFailure::Store(failure)) => AlarmReceiptResult::Error {
            message: failure.to_string(),
            failure: Some(failure.clone()),
        },
        Err(AlarmFailure::Platform(failure)) => AlarmReceiptResult::Error {
            message: failure.to_string(),
            failure: None,
        },
    };
    storage
        .put(
            RECEIPT_KEY,
            &AlarmReceipt {
                organization,
                initialized_cold,
                alarm_before,
                alarm_after,
                result,
            },
        )
        .await?;
    for waiter in std::mem::take(&mut *object.controls.alarm_waiters.borrow_mut()) {
        let _ = waiter.send(());
    }
    match outcome {
        Ok(_) => {}
        Err(AlarmFailure::Store(failure)) => return Err(error(failure)),
        Err(AlarmFailure::Platform(failure)) => return Err(failure),
    }
    Response::ok("alarm applied")
}
