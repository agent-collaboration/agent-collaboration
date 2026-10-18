#![allow(clippy::expect_used)]

use collaboration_central_conformance::{JwksFixtureServer, fixture_policy, owner_assertion};
use collaboration_central_core::{
    CopyCall, CopyMigrationResult, CopyOpsCall, CopyResult, ObjectAddress, WireCall, WireResult,
};
use collaboration_central_local::{LocalServer, LocalSettings};
use collaboration_replication_protocol::*;
use serde_json::json;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000001";
const TRANSFER: &str = "019f0000-0000-7000-8000-000000000002";

fn retired_fixture(path: &std::path::Path) {
    let receipt = json!({"transferId":TRANSFER,"fence":2,"acked":{"epoch":1,"commitId":7},"retiredAt":"2026-10-16T10:00:00Z"});
    let status = std::process::Command::new("python3").args(["-c",
        "import sqlite3,sys; c=sqlite3.connect(sys.argv[1]); c.executescript(sys.argv[3]); c.execute('INSERT INTO _copy_state VALUES(1,1,?)',(sys.argv[2],)); c.commit(); c.close()"
    ]).arg(path).arg(receipt.to_string()).arg(
        "CREATE TABLE _copy_state(singleton INTEGER PRIMARY KEY,retired INTEGER NOT NULL CHECK(retired IN(0,1)),retire_receipt TEXT); CREATE TABLE _copy_migrations(version INTEGER PRIMARY KEY,applied_at TEXT NOT NULL,from_version INTEGER NOT NULL); INSERT INTO _copy_migrations VALUES(999,'2026-10-16T10:00:00Z',998);"
    ).status().expect("minimal retired fixture writer");
    assert!(status.success());
}

fn empty_import() -> ProjectImportPage {
    let project = ProjectId::new(PROJECT).expect("project");
    let head = ReplicationPosition::new(Epoch::new(1).expect("epoch"), 0.into());
    let schema =
        SchemaIdentity::new(0, schema_fingerprint(&[]).expect("fingerprint")).expect("schema");
    let header = ImportSnapshotHeader {
        project: project.clone(),
        head,
        schema: schema.clone(),
        schema_steps: vec![],
    };
    let summary = ProjectSummary {
        status: "imported".into(),
        latest_record_at: chrono::DateTime::UNIX_EPOCH,
    };
    ProjectImportPage {
        digest: ImportDigestBuilder::new(&header)
            .expect("header")
            .finish(&summary)
            .expect("digest"),
        index: 0,
        page: SnapshotPage {
            project,
            position: head,
            schema,
            schema_steps: vec![],
            table: String::new(),
            rows: vec![],
            next: None,
            as_of: chrono::DateTime::UNIX_EPOCH,
        },
        final_summary: Some(summary),
    }
}

#[tokio::test]
async fn owner_retired_health_repair_import_and_migrate_use_the_minimal_receipt_across_restart() {
    let root = tempfile::tempdir().expect("root");
    retired_fixture(&root.path().join(format!("copy-{PROJECT}.sqlite")));
    let jwks = JwksFixtureServer::start().await.expect("JWKS");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("server");
    let http = reqwest::Client::new();
    let owner = owner_assertion().expect("owner");
    let health = http
        .get(format!(
            "{}/admin/v1/copies/{PROJECT}/health",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("retired Health");
    let health_status = health.status();
    let health_body = health.json::<serde_json::Value>().await;
    let repair = http
        .post(format!(
            "{}/admin/v1/copies/{PROJECT}/repair",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("retired Repair");
    let repair_status = repair.status();
    let repair_body = repair.json::<serde_json::Value>().await;
    let imported = http
        .post(format!(
            "{}/admin/v1/import/projects/{PROJECT}",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .json(&empty_import())
        .send()
        .await
        .expect("retired import");
    let import_status = imported.status();
    let import_body = imported.json::<serde_json::Value>().await;
    let migrated = server
        .runtime()
        .invoke(
            ObjectAddress::Copy(ProjectId::new(PROJECT).expect("project")),
            WireCall::Copy(CopyCall::Ops(CopyOpsCall::Migrate {})),
        )
        .await;
    server.shutdown().await.expect("first shutdown");
    let server = LocalServer::start(settings())
        .await
        .expect("physical restart");
    let repeated = http
        .get(format!(
            "{}/admin/v1/copies/{PROJECT}/health",
            server.base_url()
        ))
        .header("cf-access-jwt-assertion", &owner)
        .send()
        .await
        .expect("repeated Health");
    let repeated_status = repeated.status();
    let repeated_body = repeated.json::<serde_json::Value>().await;
    server.shutdown().await.expect("second shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
    let expected =
        json!({"kind":"retired","value":{"transfer":TRANSFER,"retiredAt":"2026-10-16T10:00:00Z"}});
    assert_eq!(health_status, reqwest::StatusCode::OK);
    assert_eq!(health_body.expect("typed Health"), expected);
    assert_eq!(repair_status, reqwest::StatusCode::GONE);
    assert_eq!(
        repair_body.expect("typed Repair"),
        json!({"kind":"retired"})
    );
    assert_eq!(import_status, reqwest::StatusCode::GONE);
    assert_eq!(
        import_body.expect("typed import"),
        json!({"kind":"retired"})
    );
    assert_eq!(
        migrated.expect("retired item"),
        WireResult::Copy(CopyResult::Migrated(CopyMigrationResult::Retired {}))
    );
    assert_eq!(repeated_status, reqwest::StatusCode::OK);
    assert_eq!(repeated_body.expect("retained timestamp"), expected);
}
