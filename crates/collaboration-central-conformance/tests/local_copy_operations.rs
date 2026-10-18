#![allow(clippy::expect_used)]

use collaboration_central_conformance::{
    JwksFixtureServer, fixture_policy, machine_assertion, owner_assertion,
};
use collaboration_central_core::{
    CopyCall, CopyResult, ObjectAddress, RegistryCall, RegistryResult, ReservationSource, WireCall,
    WireResult,
};
use collaboration_central_local::{LocalServer, LocalSettings};
use collaboration_replication_protocol::{
    BookkeepingKind, CopyUnavailableReason, InstallationId, ProjectId, SessionAnswer,
};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000001";

fn measured_files(root: &std::path::Path) -> std::io::Result<u64> {
    let mut size = 0;
    for name in [
        format!("copy-{PROJECT}.sqlite"),
        format!("copy-{PROJECT}.sqlite-wal"),
    ] {
        match std::fs::metadata(root.join(name)) {
            Ok(metadata) => size += metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(size)
}

#[tokio::test]
async fn owner_health_uses_target_measurement_before_first_migration_and_refuses_machine_access() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server");
    let http = reqwest::Client::new();
    let url = format!("{}/admin/v1/copies/{PROJECT}/health", server.base_url());
    let owner = owner_assertion().expect("owner");
    let first = http
        .get(&url)
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("first Health");
    let first_status = first.status();
    let first_body = first.json::<serde_json::Value>().await;
    let expected_bytes =
        measured_files(root.path()).expect("independent database file measurement");
    let repeated = http
        .get(format!("{url}?storage_bytes=1"))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("repeat Health");
    let repeated_status = repeated.status();
    let repeated_body = repeated.json::<serde_json::Value>().await;
    let machine = http
        .get(&url)
        .header(
            "cf-access-jwt-assertion",
            machine_assertion("health-machine").expect("machine assertion"),
        )
        .send()
        .await
        .expect("machine Health")
        .status();
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    assert_eq!(first_status, reqwest::StatusCode::OK);
    let first = first_body.expect("typed Health JSON");
    assert_eq!(first["kind"], "live");
    let first = &first["value"];
    assert_eq!(
        first["bookkeeping"],
        serde_json::json!({"kind":"known","value":{"kind":"uninitialized"}})
    );
    assert_eq!(
        first["copy"],
        serde_json::json!({"kind":"known","value":null})
    );
    assert_eq!(repeated_status, reqwest::StatusCode::OK);
    assert_eq!(
        repeated_body.expect("repeat JSON")["value"]["storageBytes"].as_u64(),
        Some(expected_bytes)
    );
    assert_eq!(machine, reqwest::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn owner_health_remains_reachable_after_a_real_late_migration_failure() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let database = root.path().join(format!("copy-{PROJECT}.sqlite"));
    let mut child=std::process::Command::new("python3").args(["-c", "import sqlite3,sys; connection=sqlite3.connect(sys.argv[1]); connection.executescript(sys.stdin.read()); connection.close()"]).arg(&database).stdin(std::process::Stdio::piped()).spawn().expect("legacy SQLite fixture writer");
    {
        use std::io::Write;
        let mut input = child.stdin.take().expect("fixture stdin");
        input
            .write_all(include_bytes!("../fixtures/preversioned-copy-v1.sql"))
            .expect("exact old schema");
        input.write_all(b"CREATE TABLE records(id INTEGER PRIMARY KEY,label TEXT NOT NULL); INSERT INTO records VALUES(7,'keep'); CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL,unexpected TEXT NOT NULL); INSERT INTO _copy_migrations VALUES(1,'2026-10-15T10:00:00Z',1,'legacy');").expect("versioned old failure fixture");
    }
    assert!(child.wait().expect("fixture writer exit").success());
    let server = LocalServer::start(LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    })
    .await
    .expect("server opens a legacy copy without migrating it");
    let result = server
        .runtime()
        .invoke(
            ObjectAddress::Copy(ProjectId::new(PROJECT).expect("project")),
            WireCall::Copy(CopyCall::CopyStatus {
                project: ProjectId::new(PROJECT).expect("project"),
            }),
        )
        .await;
    let expected_bytes =
        measured_files(root.path()).expect("independent database file measurement");
    let response = reqwest::Client::new()
        .get(format!(
            "{}/admin/v1/copies/{PROJECT}/health",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", owner_assertion().expect("owner"))
        .send()
        .await
        .expect("Health while migration failing");
    let status = response.status();
    let body = response.json::<serde_json::Value>().await;
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    assert_eq!(
        result.expect("typed migration failure"),
        WireResult::Copy(CopyResult::Unavailable(
            CopyUnavailableReason::MigrationPending {
                kind: BookkeepingKind::Copy,
                version: 2
            }
        ))
    );
    assert_eq!(status, reqwest::StatusCode::OK);
    let body = body.expect("Health JSON");
    assert_eq!(body["kind"], "live");
    let body = &body["value"];
    assert_eq!(
        body["bookkeeping"],
        serde_json::json!({"kind":"known","value":{"kind":"recorded","version":1}})
    );
    assert_eq!(body["migrationFailure"]["kind"], "known");
    assert_eq!(body["migrationFailure"]["value"]["version"], 2);
    assert!(
        body["migrationFailure"]["value"]["reason"]
            .as_str()
            .expect("failure reason")
            .contains("unexpected")
    );
    assert_eq!(body["storageBytes"].as_u64(), Some(expected_bytes));
}

#[tokio::test]
async fn owner_preview_reports_real_copy_migration_failure_as_503_then_recovers_after_restart() {
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let root = tempfile::tempdir().expect("root");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let initialized = server
        .runtime()
        .invoke(
            ObjectAddress::Registry,
            WireCall::Registry(RegistryCall::ReserveFirstEpoch {
                project: ProjectId::new(PROJECT).expect("project"),
                source: ReservationSource::FleetAcceptance {
                    installation: InstallationId::new("019f0000-0000-7000-8000-000000000002")
                        .expect("machine"),
                },
            }),
        )
        .await
        .expect("initialize");
    server
        .shutdown()
        .await
        .expect("shutdown before fixture mutation");
    assert!(matches!(
        initialized,
        WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease { .. }))
    ));
    let database = root.path().join(format!("copy-{PROJECT}.sqlite"));
    let mutate = |sql: &str| {
        let status = std::process::Command::new("python3").args(["-c",
            "import sqlite3,sys; connection=sqlite3.connect(sys.argv[1]); connection.executescript(sys.argv[2]); connection.close()"
        ]).arg(&database).arg(sql).status().expect("offline fixture writer");
        assert!(status.success());
    };
    // A recorded v1 with an already-present v2 column fails its actual ALTER, before stamping v2.
    mutate("DELETE FROM _copy_migrations WHERE version=2;");
    let server = LocalServer::start(settings())
        .await
        .expect("failing copy server");
    let owner = owner_assertion().expect("owner");
    let response = reqwest::Client::new()
        .post(format!(
            "{}/admin/v1/projects/{PROJECT}/takeover/preview",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("preview");
    let failed_status = response.status();
    let failed_body = response.json::<serde_json::Value>().await;
    server.shutdown().await.expect("shutdown failed server");
    mutate("INSERT INTO _copy_migrations VALUES(2,'2026-10-16T10:00:00Z',1);");
    let server = LocalServer::start(settings())
        .await
        .expect("repaired copy server");
    let response = reqwest::Client::new()
        .post(format!(
            "{}/admin/v1/projects/{PROJECT}/takeover/preview",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("repaired preview");
    let repaired_status = response.status();
    let repaired_body = response.json::<serde_json::Value>().await;
    server.shutdown().await.expect("shutdown repaired server");
    jwks.shutdown().await.expect("JWKS shutdown");
    assert_eq!(failed_status, reqwest::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        failed_body.expect("typed failure"),
        serde_json::json!({
            "kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"kind":"copy","version":2}}
        })
    );
    assert_eq!(repaired_status, reqwest::StatusCode::OK);
    assert_eq!(repaired_body.expect("preview")["kind"], "preview");
}
