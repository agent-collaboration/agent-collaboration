//! The signing key is RFC8032's public test key, never a deployment secret.
use crate::{CloudflareEd25519ClientSigner, CloudflareServiceFormTransport};
use collaboration_central_core::{
    ClientAuthenticationProbe, IssuerKeyId, ServiceClientId, identity_contract::ResourceUri,
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
    let key_id = IssuerKeyId::try_from("rfc8032-test-1".to_owned())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let secret =
        include_bytes!("../../collaboration-central-core/tests/fixtures/rfc8032-test1.pkcs8");
    let signer = CloudflareEd25519ClientSigner::from_pkcs8(key_id, secret)
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let input: ClientAuthenticationProbe = request.json().await?;
    let outcomes = observe_client_authentication(
        signer,
        CloudflareServiceFormTransport,
        client_id,
        endpoint,
        input,
    )
    .await
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    Response::from_json(&outcomes)
}
