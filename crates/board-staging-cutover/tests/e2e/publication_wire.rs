//! Capture the actual publisher's Fleet/Status requests without touching a hosted target.
use super::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::native_fixture,
    offline_cli_fixture::{RunFixture, TestResult},
};
use axum::{
    body::{Bytes, to_bytes},
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, Uri, Version},
    response::{IntoResponse, Response},
};
use std::sync::{Arc, Mutex};

struct WireRequest {
    method: Method,
    uri: Uri,
    version: Version,
    headers: HeaderMap,
    body: Bytes,
}
type Capture = Arc<Mutex<Vec<WireRequest>>>;

async fn capture(State(captured): State<Capture>, request: Request) -> Response {
    let (mut parts, body) = request.into_parts();
    let Ok(body) = to_bytes(body, 2_097_152).await else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let fleet = parts.uri.path() == "/admin/v1/import/fleet";
    let status = matches!(
        serde_json::from_slice::<collaboration_central_core::AdminProjectImportRequest>(&body),
        Ok(collaboration_central_core::AdminProjectImportRequest::Status { .. })
    );
    for name in ["cf-access-token", "cf-access-jwt-assertion"] {
        if let Some(value) = parts.headers.get_mut(name) {
            value.set_sensitive(true);
        }
    }
    let Ok(mut entries) = captured.lock() else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    entries.push(WireRequest {
        method: parts.method,
        uri: parts.uri,
        version: parts.version,
        headers: parts.headers,
        body,
    });
    drop(entries);
    if fleet {
        axum::Json(fleet_admission::FleetImportResult::AlreadyImported).into_response()
    } else if status {
        axum::Json(
            collaboration_central_core::AdminProjectImportAnswer::Refused(
                collaboration_central_core::AdminImportRefusal::NotFound,
            ),
        )
        .into_response()
    } else {
        StatusCode::TOO_MANY_REQUESTS.into_response()
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assert exact local wire shape without emitting auth values."
)]
async fn compiled_fleet_and_status_share_http_transport_and_buffered_json_framing() -> TestResult {
    // Arrange a capture-only loopback edge; synthetic replies are not publication proof.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let target = format!("http://{}", listener.local_addr()?);
    let captured = Capture::default();
    let app = axum::Router::new()
        .fallback(capture)
        .with_state(captured.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _stopped = stopped.await;
            })
            .await
    });
    let result = async {
        let host = CutoverHostFixture::new(None).map_err(|error| std::io::Error::other(error.to_string()))?;
        let source = native_fixture(&host).await.map_err(|error| std::io::Error::other(error.to_string()))?;
        let area = tempfile::tempdir()?;
        let run = RunFixture::new(&host, &source, &target, area.path())?;
        // Act through the real compiled helper and actual StagingPublisher client.
        let output = run.execute().await?;
        assert!(!output.status.success());
        let requests = captured.lock().map_err(|_| "capture poisoned")?;
        let [fleet, status, _begin] = requests.as_slice() else { return Err("expected Fleet, Status, then controlled Begin refusal".into()); };
        // Assert only path/body differ: same client, Host, method, protocol, auth and framing.
        for request in [fleet, status] {
            assert_eq!(request.method, Method::POST);
            assert_eq!(request.version, Version::HTTP_11);
            assert_eq!(request.headers.get("content-type").and_then(|value| value.to_str().ok()), Some("application/json"));
            assert_eq!(request.headers.get("content-length").and_then(|value| value.to_str().ok()).and_then(|value| value.parse::<usize>().ok()), Some(request.body.len()));
            assert!(!request.headers.contains_key("transfer-encoding"));
            assert!(!request.headers.contains_key("expect"));
        }
        assert_eq!(fleet.headers.get("host"), status.headers.get("host"));
        assert!(fleet.headers.get("cf-access-jwt-assertion") == status.headers.get("cf-access-jwt-assertion"));
        assert!(fleet.headers.contains_key("cf-access-jwt-assertion"));
        let plan: serde_json::Value = serde_json::from_slice(&std::fs::read(&run.plan)?)?;
        let fleet_body: serde_json::Value = serde_json::from_slice(&fleet.body)?;
        assert_eq!(fleet_body, plan["projects"][0]["fleet"]);
        assert_eq!(fleet.uri.path(), "/admin/v1/import/fleet");
        assert_eq!(status.uri.path(), format!("/admin/v1/import/projects/{}", source.selected.project.as_str()));
        let status_body: collaboration_central_core::AdminProjectImportRequest = serde_json::from_slice(&status.body)?;
        assert!(matches!(status_body, collaboration_central_core::AdminProjectImportRequest::Status { .. }));
        println!("PUBLICATION_WIRE_CAPTURE {}", serde_json::to_string(&serde_json::json!({
            "method":"POST", "version":"HTTP/1.1", "sameHost":true, "sameAuth":true,
            "contentType":"application/json", "transferEncoding":false, "expectContinue":false,
            "fleetBytes":fleet.body.len(), "statusBytes":status.body.len(),
            "authMode":"fixture owner assertion; hosted owner-access uses cf-access-token via the same builder",
            "fleetHeaderNames":fleet.headers.keys().map(|name|name.as_str()).collect::<Vec<_>>(),
            "statusHeaderNames":status.headers.keys().map(|name|name.as_str()).collect::<Vec<_>>()
        }))?);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _stopped = stop.send(());
    server.await??;
    result
}
