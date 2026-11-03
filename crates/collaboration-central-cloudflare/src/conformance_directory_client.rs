//! Configured directory-call addresses with the real Worker signing and HTTP.
use crate::{CloudflareServiceHttpTransport, conformance_client_key::public_fixture_signer};
use collaboration_central_core::{
    DirectoryClientProbe, ServiceClientId,
    identity_contract::{CentralRecord, ResourceUri},
    observe_directory_client,
};
use worker::{Env, Method, Request, Response, Result};

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    if request.method() != Method::Post {
        return Response::error("POST required", 405);
    }
    let client_id =
        ServiceClientId::try_from(env.var("CONFORMANCE_DIRECTORY_CLIENT_ID")?.to_string())
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let endpoint =
        ResourceUri::try_from(env.var("CONFORMANCE_DIRECTORY_TOKEN_ENDPOINT")?.to_string())
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let central: CentralRecord =
        serde_json::from_str(&env.var("CONFORMANCE_DIRECTORY_CENTRAL_RECORD")?.to_string())
            .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let signer = public_fixture_signer()
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let input: DirectoryClientProbe = request.json().await?;
    let outcomes = observe_directory_client(
        signer,
        CloudflareServiceHttpTransport,
        client_id,
        endpoint,
        &central,
        input,
    )
    .await
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    Response::from_json(&outcomes)
}
