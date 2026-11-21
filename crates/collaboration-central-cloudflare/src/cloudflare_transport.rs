//! Durable Object addressing and byte transport, without caller policy.

use crate::object_location::ObjectAddress;
use fleet_rpc::CallFailure;
use serde::{Serialize, de::DeserializeOwned};
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
    workspace_exchange::<_, crate::store_wire::StoreAnswer>(
        env, workspace, "/store", &call, fixture,
    )
    .await
    .map_err(|failure| match failure {
        CallFailure::NeverReached => StoreFailure::Unavailable,
        CallFailure::AnswerLost => StoreFailure::AnswerLost,
    })?
}

pub(crate) async fn workspace_exchange<Body: Serialize, Reply: DeserializeOwned>(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    path: &str,
    call: &Body,
    fixture: Option<&str>,
) -> std::result::Result<Reply, CallFailure> {
    let address = ObjectAddress::WorkspaceStore(workspace);
    let stub = stub(env, &address).map_err(|_| CallFailure::NeverReached)?;
    let body = serde_json::to_vec(call).map_err(|_| CallFailure::NeverReached)?;
    let headers = Headers::new();
    address_header(&headers, &address).map_err(|_| CallFailure::NeverReached)?;
    if let Some(fixture) = fixture {
        headers
            .set("x-workspace-fixture", fixture)
            .map_err(|_| CallFailure::NeverReached)?;
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
    let request = Request::new_with_init(&format!("https://central.internal{path}"), &init)
        .map_err(|_| CallFailure::NeverReached)?;
    let mut response = stub
        .fetch_with_request(request)
        .await
        .map_err(|_| CallFailure::AnswerLost)?;
    if response.status_code() != 200 {
        return Err(CallFailure::AnswerLost);
    }
    response
        .json::<Reply>()
        .await
        .map_err(|_| CallFailure::AnswerLost)
}

#[cfg(feature = "test-support")]
pub(crate) async fn call_failing_workspace_store(
    env: &Env,
    workspace: workspace_ids::WorkspaceId,
    call: crate::store_wire::OwnedStoreCall,
) -> std::result::Result<central_storage::StoreReply, central_storage::StoreFailure> {
    store_call(env, workspace, call, Some("failingBusiness")).await
}
