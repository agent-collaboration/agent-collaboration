//! Real Workerd probes for the internal workspace RPC route's address checks.
use crate::cloudflare_transport::{address_header, stub};
use crate::object_location::ObjectAddress;
use fleet_contracts::WorkspaceRequest;
use fleet_rpc::Envelope;
use serde::Deserialize;
use worker::{Env, Headers, Method, Request, RequestInit, Response, Result, Stub, js_sys};
use workspace_ids::WorkspaceId;

#[derive(Deserialize)]
pub(super) struct AddressProbe {
    physical: WorkspaceId,
    header: Option<ObjectAddress>,
    envelope: Envelope<WorkspaceId, WorkspaceRequest>,
}

pub(super) async fn probe(env: &Env, probe: AddressProbe) -> Result<Response> {
    let owner = ObjectAddress::WorkspaceStore(probe.physical);
    let target = stub(env, &owner)?;
    let before = storage_state(&target, &owner).await?;
    let headers = Headers::new();
    if let Some(header) = &probe.header {
        address_header(&headers, header)?;
    }
    let body = serde_json::to_vec(&probe.envelope)?;
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_headers(headers)
        .with_body(Some(js_sys::Uint8Array::from(body.as_slice()).into()));
    let result = target
        .fetch_with_request(Request::new_with_init(
            "https://central.internal/workspace-rpc",
            &init,
        )?)
        .await;
    let (status, body) = match result {
        Ok(mut response) => (response.status_code(), response.text().await?),
        Err(error) => (0, error.to_string()),
    };
    let after = storage_state(&target, &owner).await?;
    Response::from_json(&serde_json::json!({
        "status": status, "body": body, "storageBefore": before, "storageAfter": after,
    }))
}

async fn storage_state(target: &Stub, owner: &ObjectAddress) -> Result<[bool; 2]> {
    let headers = Headers::new();
    address_header(&headers, owner)?;
    let mut init = RequestInit::new();
    init.with_headers(headers);
    target
        .fetch_with_request(Request::new_with_init(
            "https://central.internal/test/storage-state",
            &init,
        )?)
        .await?
        .json()
        .await
}
