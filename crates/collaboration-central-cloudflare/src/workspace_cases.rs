//! Controls the actual modern CentralObject only in test-support bundles.
use super::{
    AlarmFailure, AlarmPolicy, CentralObject, ObjectAddress, OrganizationId, statement_store,
};
use crate::store_wire::{OwnedBatch, OwnedStatement, OwnedStoreCall};
use collaboration_central_core::{HeldPause, PausePoint};
use serde::{Deserialize, Serialize};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    num::NonZeroU32,
};
use worker::{Env, Request, Response, Result};
use worker::{Headers, Method, RequestInit, js_sys};

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) alarm_batch_failure:
        std::sync::Arc<std::sync::Mutex<Option<central_storage::StoreFailure>>>,
    pub(super) lose_read: std::sync::Arc<std::sync::atomic::AtomicBool>,
    observations: RefCell<BTreeMap<workspace_ids::WorkspaceId, HeldPause>>,
    holds: RefCell<BTreeMap<(workspace_ids::WorkspaceId, Point), HeldPause>>,
    central_holds: RefCell<BTreeMap<Point, HeldPause>>,
    alarm_waiters: RefCell<Vec<futures_channel::oneshot::Sender<()>>>,
    pub(super) mail_policy: Cell<Option<collaboration_protocol::central_mail::MailPolicy>>,
}
#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
enum Point {
    AfterReads,
    AfterStoreApplied,
    BeforeDrainCommit,
    AfterDrainCommit,
    BeforeTimerCommit,
    AfterTimerCommit,
    BeforeSignInSample,
    AfterSignInSample,
}
impl From<Point> for PausePoint {
    fn from(point: Point) -> Self {
        match point {
            Point::AfterReads => Self::AfterReads,
            Point::AfterStoreApplied => Self::AfterStoreApplied,
            Point::BeforeDrainCommit => Self::BeforeDrainCommit,
            Point::AfterDrainCommit => Self::AfterDrainCommit,
            Point::BeforeTimerCommit => Self::BeforeTimerCommit,
            Point::AfterTimerCommit => Self::AfterTimerCommit,
            Point::BeforeSignInSample => Self::BeforeSignInSample,
            Point::AfterSignInSample => Self::AfterSignInSample,
        }
    }
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
    let path = if operation == "request" {
        "/request".to_owned()
    } else if operation == "mail" {
        "/mail".to_owned()
    } else {
        format!("/test/{operation}")
    };
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
    // Holding time before initialization also pins the first minted identity.
    if path == "/test/clock" {
        object.clock.hold(request.json::<i64>().await?);
        return Response::from_json(&true);
    }
    if path == "/test/policy" {
        let seconds = NonZeroU32::new(request.json::<u32>().await?)
            .ok_or_else(|| error("message interval seconds must be nonzero"))?;
        if object.runtime.lock().await.is_some() {
            return Err(error("runtime already initialized"));
        }
        object.controls.mail_policy.set(Some(
            collaboration_protocol::central_mail::MailPolicy::new(seconds),
        ));
        return Response::from_json(&true);
    }
    let runtime = object.runtime(organization).await?;
    match path.as_str() {
        "/test/lose-provisioning-reply" => {
            runtime.lose_next_provisioning_reply();
            Response::from_json(&true)
        }
        "/test/fail-alarm-batch" => {
            let failure = request.json().await?;
            *object
                .controls
                .alarm_batch_failure
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(failure);
            Response::from_json(&true)
        }
        "/test/drain" => {
            let workspace = request.json().await?;
            Response::from_json(&runtime.drain_workspace(&workspace).await)
        }
        "/test/due-batch" => {
            let (session, through): (workspace_model::SessionAddress, workspace_model::Timestamp) =
                request.json().await?;
            Response::from_json(&runtime.due_for_session(&session, through).await)
        }
        "/test/alarm" => Response::from_json(&object.state.storage().get_alarm().await?),
        "/test/hold-central" => {
            let point: Point = request.json().await?;
            if !matches!(
                point,
                Point::BeforeTimerCommit
                    | Point::AfterTimerCommit
                    | Point::BeforeSignInSample
                    | Point::AfterSignInSample
            ) {
                return Err(error("not a timer point"));
            }
            object
                .controls
                .central_holds
                .borrow_mut()
                .insert(point, runtime.pause_points().hold_central(point.into()));
            Response::from_json(&true)
        }
        "/test/until-central-waiting" => {
            let count: usize = request.json().await?;
            runtime.until_central_waiting(count).await;
            Response::from_json(&true)
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
        "/test/central-batch" => Response::from_json(
            &statement_store::run_batch(
                &object.connection,
                request.json::<OwnedBatch>().await?,
                AlarmPolicy::Ignore,
            )
            .await,
        ),
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
                std::sync::Arc::new(
                    collaboration_central_core::workspace_migrations::failing_upgrade(
                        collaboration_central_core::workspace_migrations::central_migrator(),
                    ),
                ),
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
            runtime.lose_reply_after_apply(request.json().await?);
            Response::from_json(&true)
        }
        "/test/hold" => {
            let target: Pause = request.json().await?;
            let held = runtime
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
                "SELECT (SELECT COUNT(*) FROM wakes WHERE state='active'), (SELECT COUNT(*) FROM wakes WHERE state='done'), (SELECT COUNT(*) FROM messages WHERE message_kind='wake')",
                "SELECT id,organization_statement,created_at,updated_at FROM organization_identity ORDER BY id",
                "SELECT * FROM organization_change_records ORDER BY id",
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
                    sql: "DELETE FROM organization_identity",
                    args: vec![],
                },
                expect: central_storage::RowExpectation::Any,
            }];
            for (id, statement) in rows {
                statements.push(central_storage::GuardedStatement {
                    statement: central_storage::Statement { sql: "INSERT INTO organization_identity(id,organization_statement,created_at,updated_at) VALUES (?,?,0,0)", args: vec![central_storage::SqlValue::Text(id), central_storage::SqlValue::Text(statement)] },
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
        runtime
            .on_alarm()
            .await
            .map(Some)
            .map_err(AlarmFailure::Store)
    }
    .await;
    let alarm_after = storage.get_alarm().await?;
    let result = match &outcome {
        Ok(Some(result)) => AlarmReceiptResult::Applied {
            wakes_fired: result.wakes_fired,
            automations_fired: result.automations_fired,
            windows_fired: result.windows_fired,
            workspaces_drained: result.workspaces_drained,
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
