//! The real Router transport decodes central's response across a fixture Access hop.
use super::*;
use axum::{
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use board_fleet_sync::{AccessServiceToken, ReqwestSessionTransport};
use fleet_sync_session::{ConnectFailure, Endpoint, PeerCredential, SessionTransport};

#[derive(Clone, Copy)]
pub(super) enum HeaderMode {
    Original,
    Missing,
    Malformed,
    Repeated,
}

pub(super) async fn rejected(
    base: &str,
    credential: &str,
    access_credential: &str,
    expected: AuthRefusal,
    mode: HeaderMode,
) -> ConformanceResult<()> {
    let upstream = format!("{base}/v1/session");
    let assertion = machine_assertion(access_credential)?;
    let app = axum::Router::new().route(
        "/v1/session",
        get(move |headers: HeaderMap| {
            let upstream = upstream.clone();
            let assertion = assertion.clone();
            async move {
                if headers
                    .get("cf-access-client-id")
                    .and_then(|v| v.to_str().ok())
                    != Some("refusal-fixture-client")
                    || headers
                        .get("cf-access-client-secret")
                        .and_then(|v| v.to_str().ok())
                        != Some("refusal-fixture-secret")
                {
                    return StatusCode::FORBIDDEN.into_response();
                }
                match forward(upstream, assertion, headers, mode).await {
                    Ok(response) => response,
                    Err(_) => StatusCode::BAD_GATEWAY.into_response(),
                }
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let endpoint = Endpoint::new(&format!("http://{}/v1/session", listener.local_addr()?))?;
    let stop = tokio_util::sync::CancellationToken::new();
    let stopped = stop.clone();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(stopped.cancelled_owned())
            .await
    });
    let transport = ReqwestSessionTransport::new(
        AccessServiceToken::new("refusal-fixture-client", "refusal-fixture-secret")?,
        Duration::from_secs(5),
        Duration::from_secs(5),
    )?;
    let result = transport
        .connect(
            &endpoint,
            &PeerCredential::new(format!("Bearer {credential}"))?,
        )
        .await;
    stop.cancel();
    task.await??;
    match result {
        Err(ConnectFailure::CredentialRejected(actual)) if actual == expected => Ok(()),
        Err(failure) => Err(format!("real Router transport lost {expected:?}: {failure:?}").into()),
        Ok(_) => Err("rejected credential acquired a Router transport".into()),
    }
}
async fn forward(
    upstream: String,
    assertion: String,
    headers: HeaderMap,
    mode: HeaderMode,
) -> ConformanceResult<Response> {
    let authorization = headers
        .get("authorization")
        .ok_or("missing fixture authorization")?
        .clone();
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()?
        .get(upstream)
        .header("authorization", authorization)
        .header("Cf-Access-Jwt-Assertion", assertion)
        .upgrade()
        .send()
        .await?;
    let status = response.status();
    let mut headers = response.headers().clone();
    match mode {
        HeaderMode::Original => {}
        HeaderMode::Missing => {
            headers.remove("fleet-sync-auth-refusal");
        }
        HeaderMode::Malformed => {
            headers.insert("fleet-sync-auth-refusal", "expired".parse()?);
        }
        HeaderMode::Repeated => {
            headers.append("fleet-sync-auth-refusal", "\"expired\"".parse()?);
        }
    }
    let body = response.into_inner().bytes().await?;
    let mut output = (status, body).into_response();
    *output.headers_mut() = headers;
    Ok(output)
}
