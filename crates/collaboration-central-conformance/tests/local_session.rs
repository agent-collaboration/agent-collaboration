#![allow(clippy::expect_used, clippy::panic)]
use collaboration_central_core::{AdminRequest, AdminResult};
use collaboration_central_local::{LocalServer, LocalSettings};
use collaboration_replication_protocol::*;
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::RequestBuilderExt;
use sha2::{Digest, Sha256};
fn hash(token: &str) -> String {
    format!("{:x}", Sha256::digest(token.as_bytes()))
}
#[tokio::test]
async fn malformed_frames_get_correlated_refusals_or_typed_physical_close() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = collaboration_central_conformance::ConformanceClient::new(server.base_url())
        .expect("client");
    let result = collaboration_central_conformance::run_malformed_frame_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    result.expect("raw-frame refusal/close scenarios");
}
#[tokio::test]
async fn authenticated_upgrade_welcome_and_replacement_run_through_real_websockets() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("directory");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = reqwest::Client::builder()
        .http1_only()
        .no_proxy()
        .build()
        .expect("client");
    let installation = InstallationId::new(uuid::Uuid::now_v7().to_string()).expect("installation");
    let request = AdminRequest::EnrollMachine {
        label: "test machine".into(),
        installation: installation.clone(),
        access_client_id: collaboration_central_conformance::access_client_id("test-machine")
            .expect("client"),
        credential_hash: hash("test-machine"),
        expires_at: chrono::Utc::now() + chrono::TimeDelta::hours(24),
    };
    let response = client
        .post(format!("{}/admin/v1/machines/enroll", server.base_url()))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion().expect("owner assertion"),
        )
        .json(&request)
        .send()
        .await
        .expect("enroll");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(
        response.json::<AdminResult>().await.expect("result"),
        AdminResult::MachineEnrolled
    );
    let denied = client
        .get(format!("{}/v1/session", server.base_url()))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion().expect("owner assertion"),
        )
        .upgrade()
        .send()
        .await
        .expect("admin on session");
    assert_eq!(denied.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(denied.into_websocket().await.is_err());
    let mut first = client
        .get(format!("{}/v1/session", server.base_url()))
        .bearer_auth("test-machine")
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion("test-machine")
                .expect("machine assertion"),
        )
        .upgrade()
        .send()
        .await
        .expect("upgrade")
        .into_websocket()
        .await
        .expect("websocket");
    let session_id = SessionId::new(uuid::Uuid::now_v7().to_string()).expect("session");
    let hello = MachineFrame::Hello {
        installation,
        session_id: session_id.clone(),
        schema_support: SchemaSupport { max_version: 10 },
    };
    first
        .send(reqwest_websocket::Message::Binary(
            serde_json::to_vec(&hello).expect("hello").into(),
        ))
        .await
        .expect("send");
    let frame = tokio::time::timeout(std::time::Duration::from_secs(10), first.next())
        .await
        .expect("welcome deadline")
        .expect("welcome")
        .expect("frame");
    let reqwest_websocket::Message::Binary(bytes) = frame else {
        panic!("binary welcome")
    };
    assert!(
        matches!(serde_json::from_slice::<CentralFrame>(&bytes).expect("welcome"),CentralFrame::Welcome{session_id:actual,projects}if actual==session_id&&projects.is_empty())
    );
    let mut second = client
        .get(format!("{}/v1/session", server.base_url()))
        .bearer_auth("test-machine")
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion("test-machine")
                .expect("machine assertion"),
        )
        .upgrade()
        .send()
        .await
        .expect("upgrade")
        .into_websocket()
        .await
        .expect("websocket");
    second
        .send(reqwest_websocket::Message::Binary(
            serde_json::to_vec(&hello).expect("hello").into(),
        ))
        .await
        .expect("send");
    let old = tokio::time::timeout(std::time::Duration::from_secs(10), first.next())
        .await
        .expect("closed deadline")
        .expect("closed")
        .expect("frame");
    let reqwest_websocket::Message::Binary(bytes) = old else {
        panic!("closed binary")
    };
    assert_eq!(
        serde_json::from_slice::<CentralFrame>(&bytes).expect("closed"),
        CentralFrame::Closed {
            reason: SessionCloseReason::ConnectionReplaced
        }
    );
    let new = tokio::time::timeout(std::time::Duration::from_secs(10), second.next())
        .await
        .expect("welcome deadline")
        .expect("welcome")
        .expect("frame");
    let reqwest_websocket::Message::Binary(bytes) = new else {
        panic!("welcome binary")
    };
    assert!(matches!(
        serde_json::from_slice::<CentralFrame>(&bytes).expect("welcome"),
        CentralFrame::Welcome { .. }
    ));
    drop(first);
    drop(second);
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}
