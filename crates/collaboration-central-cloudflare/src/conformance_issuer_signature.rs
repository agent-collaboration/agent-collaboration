//! Primitive-only probe: no Access claim, clock, ID, storage or network decision.
use crate::{
    access_verification::{InvocationLabels, VerificationLayer},
    platform::CloudflareCentralPlatform,
};
use collaboration_central_core::{IssuerSignatureProbe, observe_issuer_signature};
use worker::{Env, Method, Request, Response, Result};

pub(crate) async fn fetch(mut request: Request, env: Env) -> Result<Response> {
    if request.method() != Method::Post {
        return Response::error("POST required", 405);
    }
    let input: IssuerSignatureProbe = request.json().await?;
    let platform = CloudflareCentralPlatform::new(
        "https://unused.example.invalid/jwks".into(),
        InvocationLabels::for_signature_probe(),
        VerificationLayer::Edge,
        env,
    );
    Response::from_json(&observe_issuer_signature(&platform, input).await)
}
