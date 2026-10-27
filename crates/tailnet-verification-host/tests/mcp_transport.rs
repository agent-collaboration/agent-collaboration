#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions fail these tests while fixture errors propagate through Result."
)]

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
};
use codex_router_tailnet_verification_host::host_plan::HttpsEndpoint;
use codex_router_tailnet_verification_host::mcp_observation::{
    LocalTarget, LoopbackEndpoint, ProbeHeaders, http_post, observe_http_boundary,
    observe_local_inventory, rpc_result,
};
use serde_json::{Value, json};

#[derive(Clone)]
struct AuthorityFixture {
    authority: String,
    local_origin: String,
    enforce: bool,
    requests: Arc<AtomicUsize>,
}
async fn mcp_response(
    State(fixture): State<AuthorityFixture>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    fixture.requests.fetch_add(1, Ordering::Relaxed);
    let payload: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let host = headers
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let origin = headers.get("origin").and_then(|value| value.to_str().ok());
    let inventory = payload.pointer("/params/name") == Some(&json!("endpoints_list"));
    let allowed_host = host.starts_with("127.0.0.1:")
        || host == fixture.authority
        || fixture.authority.strip_suffix(":443") == Some(host);
    let mut status = StatusCode::OK;
    if payload.get("jsonrpc") != Some(&json!("2.0"))
        || payload.get("id") != Some(&json!(1))
        || !(payload.get("method") == Some(&json!("tools/list"))
            || (payload.get("method") == Some(&json!("tools/call")) && inventory))
    {
        status = StatusCode::BAD_REQUEST;
    }
    if (inventory && host != fixture.authority)
        || (fixture.enforce
            && (!allowed_host || origin.is_some_and(|origin| origin != fixture.local_origin)))
    {
        status = StatusCode::FORBIDDEN;
    }
    let result = if inventory {
        json!({"structuredContent":{"serviceEpoch":"epoch","endpoints":[{"endpoint":{"serviceId":"destination","endpointId":"codex-local"},"availability":{"state":"available"},"channels":[{"kind":"nativeCodex","generation":{"serviceEpoch":"epoch","generation":1}}]}]}})
    } else {
        json!({"tools":[]})
    };
    (
        status,
        Json(json!({"jsonrpc":"2.0","id":1,"result":result})),
    )
}

async fn exercise_boundary(
    remote_port: u16,
    enforce: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let requests = Arc::new(AtomicUsize::new(0));
    let authority = format!("node.tail.ts.net:{remote_port}");
    let application =
        Router::new()
            .route("/mcp", post(mcp_response))
            .with_state(AuthorityFixture {
                authority: authority.clone(),
                local_origin: format!("http://127.0.0.1:{port}"),
                enforce,
                requests: Arc::clone(&requests),
            });
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, application)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let result = tokio::task::spawn_blocking(move || -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let local = LoopbackEndpoint::parse(format!("http://127.0.0.1:{port}/mcp"))?; let remote = HttpsEndpoint { url: format!("https://{authority}/mcp") };
        let observed = observe_http_boundary(&local, &remote);
        if enforce {
            let statuses = observed?;
            assert_eq!(statuses.get("proxyAuthorityControl"), Some(&json!(200)));
            assert_eq!(statuses.get("tailnetOrigin"), Some(&json!(403)));
            assert_eq!(statuses.get("wrongPort"), Some(&json!(403)));
            assert_eq!(statuses.get("missingPort"), Some(&json!(if remote_port == 443 {200} else {403})));
            let inventory = observe_local_inventory(&local, &remote, ("destination", "epoch"))?;
            assert_eq!(inventory.get("serviceEpoch"), Some(&json!("epoch")));
            let missing_host = http_post(LocalTarget::Loopback(&local), &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"endpoints_list","arguments":{}}}), &ProbeHeaders::default())?;
            assert_eq!(missing_host.status, 403);
        } else { assert!(observed.is_err(), "permissive listener must fail the boundary proof"); }
        Ok(())
    }).await?;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    result?;
    assert_eq!(
        requests.load(Ordering::Relaxed),
        if enforce { 11 } else { 5 }
    );
    Ok(())
}

#[tokio::test]
async fn real_tcp_authority_and_origin_probes_match_original_fixture_contract()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_boundary(8443, true).await?;
    exercise_boundary(443, true).await?;
    exercise_boundary(8443, false).await?;
    Ok(())
}

#[tokio::test]
async fn real_unix_transport_reaches_literal_mcp_response()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let parent = if cfg!(target_os = "macos") {
        "/private/tmp"
    } else {
        "/tmp"
    };
    let root = tempfile::Builder::new()
        .prefix("2026-10-mcp-")
        .tempdir_in(parent)?;
    let socket = root.path().join("mcp.sock");
    let listener = tokio::net::UnixListener::bind(&socket)?;
    let fixture = AuthorityFixture {
        authority: "unused".to_owned(),
        local_origin: "unused".to_owned(),
        enforce: false,
        requests: Arc::new(AtomicUsize::new(0)),
    };
    let application = Router::new()
        .route("/mcp", post(mcp_response))
        .with_state(fixture);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, application)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
    });
    let result = tokio::task::spawn_blocking(
        move || -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
            Ok(rpc_result(http_post(
                LocalTarget::Unix(&socket),
                &json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}),
                &ProbeHeaders::default(),
            )?)?)
        },
    )
    .await?;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server).await???;
    assert_eq!(result?, json!({"tools":[]}));
    Ok(())
}
