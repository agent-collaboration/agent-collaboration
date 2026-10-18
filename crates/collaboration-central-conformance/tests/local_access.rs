#![allow(clippy::expect_used)]
use collaboration_central_conformance::{
    ConformanceClient, FIXTURE_KID, JwksFixtureServer, access_client_id, fixture_claims,
    fixture_policy, owner_assertion, run_access_conformance,
    run_access_verifier_control_conformance, sign_assertion,
};
use collaboration_central_core::AccessCaller;
use collaboration_central_local::{LocalServer, LocalSettings};
#[tokio::test]
async fn valid_2048_and_tampered_controls_do_not_admit_a_valid_1024_bit_signature() {
    use collaboration_central_conformance::{
        AccessCallerClass, AccessVerificationOutcome, fixture_jwks, flip_signature_byte,
    };
    use collaboration_central_core::{AccessJwk, AccessRefusal};
    #[derive(serde::Deserialize)]
    struct WeakFixture {
        key: AccessJwk,
        assertion: String,
    }
    let weak: WeakFixture = serde_json::from_str(include_str!("../fixtures/access-weak-1024.json"))
        .expect("weak fixture");
    let mut keys = fixture_jwks().expect("primary fixture");
    keys.keys.push(weak.key);
    let jwks = JwksFixtureServer::start_with_keys(keys)
        .await
        .expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let owner = owner_assertion().expect("primary assertion");
    let cases = [
        (
            owner.clone(),
            AccessVerificationOutcome::Accepted(AccessCallerClass::Owner),
        ),
        (
            flip_signature_byte(&owner).expect("tamper"),
            AccessVerificationOutcome::Refused(AccessRefusal::BadSignature),
        ),
        (
            weak.assertion,
            AccessVerificationOutcome::Refused(AccessRefusal::InvalidKey),
        ),
    ];
    let mut observations = Vec::new();
    for (assertion, expected) in cases {
        observations.push((
            client
                .verify_access_assertion(assertion, "/admin/v1/machines/enroll")
                .await,
            expected,
        ));
    }
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    for (actual, expected) in observations {
        assert_eq!(actual.expect("actual verifier"), expected);
    }
}
#[tokio::test]
async fn signed_access_assertions_gate_real_native_http_and_websockets() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let proof = run_access_conformance(&client, &jwks.requests).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("Access cases");
}
#[tokio::test]
async fn verifier_control_runs_the_edge_verifier_on_submitted_assertions() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let proof = run_access_verifier_control_conformance(&client).await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("verifier control cases");
}
#[tokio::test]
async fn expiry_row_refuses_the_same_assertion_after_its_exp() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS server");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let client = ConformanceClient::new(server.base_url()).expect("client");
    let mut short = fixture_claims(
        AccessCaller::Machine {
            client_id: access_client_id("expiry-row-machine").expect("client ID"),
        },
        false,
    );
    short
        .as_object_mut()
        .expect("claims object")
        .insert("exp".into(), (chrono::Utc::now().timestamp() + 3).into());
    let proof = collaboration_central_conformance::run_staging_expiry_row(
        &client,
        sign_assertion(&short, FIXTURE_KID).expect("short-lived assertion"),
        owner_assertion().expect("long-lived assertion"),
    )
    .await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    proof.expect("expiry row");
}
