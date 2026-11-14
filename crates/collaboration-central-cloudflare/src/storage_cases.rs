//! Test-support routes for the storage boundary's workerd cases (Verification O1).
//! Built only with the non-default `test-support` feature; the production bundle
//! check proves these routes absent.

#[path = "storage_cases/case_object.rs"]
mod case_object;
#[path = "storage_cases/case_steps.rs"]
mod case_steps;

pub use case_object::StorageCaseObject;

use case_steps::CaseStoreCall;
use central_storage::StoreCall;
use worker::{Env, Headers, Method, Request, RequestInit, Response, Result, js_sys};

const STORAGE_PREFIX: &str = "/test-support/storage/";
const CALL_STORE_PREFIX: &str = "/test-support/call-store/";
const DRIVER_PREFIX: &str = "/test-support/driver/";

/// Answers a test-support route, or returns the request for the central router.
pub(crate) async fn route(
    request: Request,
    env: &Env,
) -> Result<std::result::Result<Response, Request>> {
    let request =
        match crate::workspace_central_object::workspace_cases::route(request, env).await? {
            Ok(response) => return Ok(Ok(response)),
            Err(request) => request,
        };
    let mut request = request;
    let path = request.path();
    #[cfg(feature = "conformance")]
    if path == "/test-support/object-clock-ids" {
        return Ok(Ok(crate::composed_control::forward(
            env,
            &collaboration_central_core::ObjectAddress::Fleet,
            "/test/object-clock-ids",
            Some(request.text().await?),
        )
        .await?));
    }
    if let Some(scenario) = path.strip_prefix(DRIVER_PREFIX) {
        return Ok(Ok(crate::driver_scenarios::route(scenario, env).await?));
    }
    if let Some(rest) = path.strip_prefix(STORAGE_PREFIX) {
        let Some((object, operation)) = rest.split_once('/') else {
            return Ok(Ok(Response::error("missing case operation", 404)?));
        };
        let headers = Headers::new();
        if let Some(migrator) = request.headers().get("x-case-migrator")? {
            headers.set("x-case-migrator", &migrator)?;
        }
        let body = request.bytes().await?;
        let mut init = RequestInit::new();
        init.with_method(Method::Post)
            .with_headers(headers)
            .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
        let forwarded =
            Request::new_with_init(&format!("https://case.internal/{operation}"), &init)?;
        let stub = env
            .durable_object("STORAGE_CASES")?
            .id_from_name(object)?
            .get_stub()?;
        return Ok(Ok(stub.fetch_with_request(forwarded).await?));
    }
    if let Some(workspace) = path.strip_prefix(CALL_STORE_PREFIX) {
        let workspace = workspace_ids::WorkspaceId::try_from(workspace.to_owned())
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
        let call = match request.json::<CaseStoreCall>().await? {
            CaseStoreCall::Batch(batch) => StoreCall::Batch(batch.into_batch()),
            CaseStoreCall::Read(read) => StoreCall::Read(read.statement()),
        };
        return Ok(Ok(Response::from_json(
            &crate::cloudflare_transport::call_widget_store(env, workspace, call.into()).await,
        )?));
    }
    Ok(Err(request))
}
