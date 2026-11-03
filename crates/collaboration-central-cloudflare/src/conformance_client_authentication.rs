//! The signing key is RFC8032's public test key, never a deployment secret.
use crate::{CloudflareServiceHttpTransport, conformance_client_key::public_fixture_signer};
use collaboration_central_core::{
    ClientAuthenticationProbe, ServiceClientId, identity_contract::ResourceUri,
    observe_client_authentication,
};
use worker::{Env, Method, Request, Response, Result};

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    if request.method() != Method::Post {
        return Response::error("POST required", 405);
    }
    let client_id = ServiceClientId::try_from(env.var("CONFORMANCE_CLIENT_ID")?.to_string())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let endpoint = ResourceUri::try_from(env.var("CONFORMANCE_CLIENT_INTROSPECTION")?.to_string())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let signer = public_fixture_signer()
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let input: ClientAuthenticationProbe = request.json().await?;
    let outcomes = observe_client_authentication(
        signer,
        CloudflareServiceHttpTransport,
        client_id,
        endpoint,
        input,
    )
    .await
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    Response::from_json(&outcomes)
}
