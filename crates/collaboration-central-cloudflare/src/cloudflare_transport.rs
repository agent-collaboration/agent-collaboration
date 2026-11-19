//! Durable Object addressing and byte transport, without caller policy.

use crate::object_location::ObjectAddress;
use worker::{Env, Headers, Method, Request, RequestInit, Result, Stub, js_sys};

pub(crate) use crate::object_location::location;
pub(crate) fn stub(env: &Env, address: &ObjectAddress) -> Result<Stub> {
    let (binding, name) = location(address);
    env.durable_object(binding)?.id_from_name(&name)?.get_stub()
}
pub(crate) fn address_header(headers: &Headers, address: &ObjectAddress) -> Result<()> {
    headers.set("x-central-object-address", &serde_json::to_string(address)?)
}
/// Carries one store call to a workspace's store object and returns its answer.
///
/// A request that never reached the object is `Unavailable`; a reply that did not
/// arrive or parse after the call may have run is `AnswerLost`.
pub(crate) async fn call_store(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    call: crate::store_wire::OwnedStoreCall,
) -> std::result::Result<central_storage::StoreReply, central_storage::StoreFailure> {
    store_call(env, workspace, call, None).await
}
#[cfg(feature = "test-support")]
pub(crate) async fn call_widget_store(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    call: crate::store_wire::OwnedStoreCall,
) -> std::result::Result<central_storage::StoreReply, central_storage::StoreFailure> {
    store_call(env, workspace, call, Some("widgets")).await
}
async fn store_call(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    call: crate::store_wire::OwnedStoreCall,
    fixture: Option<&str>,
) -> std::result::Result<central_storage::StoreReply, central_storage::StoreFailure> {
    use central_storage::StoreFailure;
    let address = ObjectAddress::WorkspaceStore(workspace);
    let stub = stub(env, &address).map_err(|_| StoreFailure::Unavailable)?;
    let body = serde_json::to_vec(&call).map_err(|_| StoreFailure::Unavailable)?;
    let headers = Headers::new();
    address_header(&headers, &address).map_err(|_| StoreFailure::Unavailable)?;
    if let Some(fixture) = fixture {
        headers
            .set("x-workspace-fixture", fixture)
            .map_err(|_| StoreFailure::Unavailable)?;
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
    let request = Request::new_with_init("https://central.internal/store", &init)
        .map_err(|_| StoreFailure::Unavailable)?;
    let mut response = stub
        .fetch_with_request(request)
        .await
        .map_err(|_| StoreFailure::AnswerLost)?;
    if response.status_code() != 200 {
        return Err(StoreFailure::AnswerLost);
    }
    response
        .json::<crate::store_wire::StoreAnswer>()
        .await
        .map_err(|_| StoreFailure::AnswerLost)?
}

#[cfg(feature = "test-support")]
pub(crate) async fn call_failing_workspace_store(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    call: crate::store_wire::OwnedStoreCall,
) -> std::result::Result<central_storage::StoreReply, central_storage::StoreFailure> {
    store_call(env, workspace, call, Some("failingBusiness")).await
}
