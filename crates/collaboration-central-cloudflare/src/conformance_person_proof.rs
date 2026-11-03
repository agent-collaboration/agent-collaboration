//! Isolated controlled-clock route; no production trust configuration is read.
use crate::{
    CloudflareIssuerKeySource,
    access_verification::{InvocationLabels, VerificationLayer},
    platform::CloudflareCentralPlatform,
};
use collaboration_central_core::{
    PersonProofProbe, identity_contract::ResourceUri, observe_person_proof,
    organization_authority::PersonTokenPolicy,
};
use worker::{Env, Method, Request, Response, Result};

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    if request.method() != Method::Post {
        return Response::error("POST required", 405);
    }
    let policy = PersonTokenPolicy::new(
        env.var("CONFORMANCE_PERSON_ISSUER")?.to_string(),
        env.var("CONFORMANCE_PERSON_ADMIN_AUDIENCE")?.to_string(),
        env.var("CONFORMANCE_PERSON_OPERATOR_AUDIENCE")?.to_string(),
        env.var("CONFORMANCE_PERSON_OPERATOR_CLIENT")?.to_string(),
    )
    .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let address = ResourceUri::try_from(env.var("CONFORMANCE_PERSON_JWKS")?.to_string())
        .map_err(|error| worker::Error::RustError(error.to_string()))?;
    let input: PersonProofProbe = request.json().await?;
    let platform = CloudflareCentralPlatform::new(
        "https://unused.example.invalid/jwks".into(),
        InvocationLabels::for_signature_probe(),
        VerificationLayer::Edge,
        env,
    );
    let source = CloudflareIssuerKeySource::new(address);
    Response::from_json(&observe_person_proof(platform, source, &policy, input).await)
}
