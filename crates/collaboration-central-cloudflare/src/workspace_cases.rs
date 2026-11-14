//! Controls the actual modern CentralObject only in test-support bundles.
use super::{AlarmPolicy, CentralObject, ObjectAddress, OrganizationId, statement_store};
use crate::store_wire::{OwnedBatch, OwnedStatement, OwnedStoreCall};
use collaboration_central_core::{HeldPause, PausePoint};
use serde::Deserialize;
use std::{cell::RefCell, collections::BTreeMap};
use worker::{Env, Request, Response, Result};
use worker::{Headers, Method, RequestInit, js_sys};

#[derive(Default)]
pub(crate) struct Controls {
    pub(super) lose_read: std::sync::Arc<std::sync::atomic::AtomicBool>,
    observations: RefCell<BTreeMap<message_board::WorkspaceId, HeldPause>>,
    holds: RefCell<BTreeMap<(message_board::WorkspaceId, Point), HeldPause>>,
}
#[derive(Clone, Copy, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(rename_all = "camelCase")]
enum Point {
    AfterReads,
    AfterStoreApplied,
}
impl From<Point> for PausePoint {
    fn from(point: Point) -> Self {
        match point {
            Point::AfterReads => Self::AfterReads,
            Point::AfterStoreApplied => Self::AfterStoreApplied,
        }
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Pause {
    workspace: message_board::WorkspaceId,
    point: Point,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Wait {
    workspace: message_board::WorkspaceId,
    count: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    workspace: message_board::WorkspaceId,
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
    let runtime = object.runtime(organization).await?;
    match path.as_str() {
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
            let workspace: message_board::WorkspaceId = request.json().await?;
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
