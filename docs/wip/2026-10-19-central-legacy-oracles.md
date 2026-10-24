# Historical local test oracles for the central hard cut

These are the original 3ca2acea test-local sources for rows13–16 and33. They
are historical evidence, not compiled or parked test code. Their old LocalServer,
wire and storage types are removed by decision508. Functional proof remains owed
at the owners in the removal map; deleting these entries grants no credit.

## crates/collaboration-central-conformance/tests/local_copy_operations.rs

Source: `3ca2acea3ed1aeb0eff0a2db21caf5794b15eb9b:crates/collaboration-central-conformance/tests/local_copy_operations.rs`.
SHA256: `c9b3e30d39dbeaf5e54273ad7c0335fb017956e6f36d9e6c4c4bb00e68907803`.

```rust
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
```

## crates/collaboration-central-conformance/tests/local_copy_retirement.rs

Source: `3ca2acea3ed1aeb0eff0a2db21caf5794b15eb9b:crates/collaboration-central-conformance/tests/local_copy_retirement.rs`.
SHA256: `52f98855a1660b4a89bf3b8654881212735cb3fcba749138a37c5f8c728b1ac5`.

```rust
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
```

## crates/collaboration-central-conformance/tests/local_storage.rs

Source: `3ca2acea3ed1aeb0eff0a2db21caf5794b15eb9b:crates/collaboration-central-conformance/tests/local_storage.rs`.
SHA256: `b7b6303bd1ab883eec1d2925e7da07747690ed460e208080ad5bc0c734c3d4cf`.

```rust
#![allow(clippy::expect_used)]
use collaboration_central_core::*;
use collaboration_central_local::ObjectActors;
use collaboration_replication_protocol::*;
fn project() -> ProjectId {
    ProjectId::new("019f0000-0000-7000-8000-000000000001").expect("project")
}
fn installation() -> InstallationId {
    InstallationId::new("019f0000-0000-7000-8000-000000000002").expect("installation")
}
fn epoch() -> Epoch {
    Epoch::new(1).expect("epoch")
}
fn now() -> RegistryTime {
    RegistryTime(chrono::Utc::now())
}
fn call(call: WireCall) -> WireInput {
    WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string()).expect("call"),
        call: Box::new(call),
    }
}

#[tokio::test]
async fn legacy_actor_refuses_a_mailbox_without_creating_a_second_store()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let root = tempfile::tempdir()?;
    let actors = ObjectActors::new(root.path())?;
    let service =
        <SessionKind as ObjectKind>::Address::try_from(installation().as_str().to_owned())?;

    let result = actors
        .step(
            ObjectAddress::Mailbox(service),
            WireInput::Alarm {
                face: CallerKind::Mailbox,
            },
            now(),
        )
        .await;

    let empty = std::fs::read_dir(root.path()).map(|mut entries| entries.next().is_none());
    actors.shutdown().await?;
    if result.is_ok() {
        return Err("the legacy actor accepted a Mailbox input".into());
    }
    if !empty? {
        return Err("the legacy actor created a second Mailbox store".into());
    }
    Ok(())
}
async fn status(actors: &ObjectActors) -> CopyStatus {
    let input = call(WireCall::Copy(CopyCall::CopyStatus { project: project() }));
    let effects = actors
        .step(ObjectAddress::Copy(project()), input.clone(), now())
        .await
        .expect("status")
        .into_trace();
    validate_call_returns(&input, &effects).expect("Return");
    effects
        .into_iter()
        .find_map(|e| match e {
            WireEffect::Return {
                result: WireResult::Copy(CopyResult::CopyStatus(Some(s))),
                ..
            } => Some(s),
            _ => None,
        })
        .expect("copy")
}
#[tokio::test]
async fn real_turso_actor_reopens_ack_and_explicitly_rolls_back_late_constraint_error() {
    let root = tempfile::tempdir().expect("directory");
    let actors = ObjectActors::new(root.path()).expect("actors");
    actors
        .step(
            ObjectAddress::Copy(project()),
            call(WireCall::Copy(CopyCall::Initialize {
                project: project(),
                epoch: epoch(),
                holder: installation(),
            })),
            now(),
        )
        .await
        .expect("initialize");
    let schema = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(),
        continues_from: ReplicationPosition::new(epoch(), 0.into()),
        through: ReplicationPosition::new(epoch(), 1.into()),
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: 1.into(),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE)".into(),
            ],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: now().0,
        }),
    })
    .expect("schema");
    actors
        .step(
            ObjectAddress::Copy(project()),
            call(WireCall::Copy(CopyCall::ApplyPiece {
                piece: schema,
                sender: installation(),
            })),
            now(),
        )
        .await
        .expect("schema apply");
    let row = |id| RowChange::Upsert {
        table: "records".into(),
        key: vec![SqlValue::Integer(id)],
        values: vec![SqlValue::Integer(id), SqlValue::Text("duplicate".into())],
    };
    let bad = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(),
        continues_from: ReplicationPosition::new(epoch(), 1.into()),
        through: ReplicationPosition::new(epoch(), 2.into()),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: 2.into(),
            rows: vec![row(1), row(2)],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: now().0,
        }),
    })
    .expect("piece");
    assert!(
        actors
            .step(
                ObjectAddress::Copy(project()),
                call(WireCall::Copy(CopyCall::ApplyPiece {
                    piece: bad,
                    sender: installation()
                })),
                now()
            )
            .await
            .is_err()
    );
    assert_eq!(
        status(&actors).await.acked,
        ReplicationPosition::new(epoch(), 1.into())
    );
    actors
        .evict(ObjectAddress::Copy(project()))
        .await
        .expect("drop actor memory");
    assert_eq!(
        status(&actors).await.acked,
        ReplicationPosition::new(epoch(), 1.into())
    );
    let read = actors
        .step(
            ObjectAddress::Copy(project()),
            call(WireCall::Copy(CopyCall::SnapshotPage { cursor: None })),
            now(),
        )
        .await
        .expect("snapshot")
        .into_trace();
    let rows = read
        .into_iter()
        .find_map(|e| match e {
            WireEffect::Return {
                result:
                    WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
                        ReadPage::ProjectCopy(ProjectCopyRead::Page(p)),
                    ))),
                ..
            } => Some(p.rows),
            _ => None,
        })
        .expect("page");
    assert!(rows.is_empty());
    actors
        .evict(ObjectAddress::Copy(project()))
        .await
        .expect("stop actor");
}

#[tokio::test]
async fn platform_alarm_survives_server_restart_and_cleans_staged_parts() {
    let jwks = collaboration_central_conformance::JwksFixtureServer::start()
        .await
        .expect("JWKS server");
    use collaboration_central_local::{LocalServer, LocalSettings};
    let root = tempfile::tempdir().expect("directory");
    let settings = || LocalSettings {
        root: root.path().into(),
        access_policy: collaboration_central_conformance::fixture_policy(),
        jwks_url: jwks.url.clone(),
    };
    let server = LocalServer::start(settings()).await.expect("start");
    let runtime = server.runtime();
    runtime
        .invoke(
            ObjectAddress::Copy(project()),
            WireCall::Copy(CopyCall::Initialize {
                project: project(),
                epoch: epoch(),
                holder: installation(),
            }),
        )
        .await
        .expect("init");
    let transaction = ReplicatedTransaction::SchemaStep {
        commit_id: 1.into(),
        migration_version: 1,
        statements: vec![
            "CREATE TABLE alarm_records(id INTEGER PRIMARY KEY)".into(),
            "CREATE INDEX alarm_idx ON alarm_records(id)".into(),
        ],
    };
    let digest = PieceDigest::of_transaction(&transaction).expect("digest");
    let part = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(),
        continues_from: ReplicationPosition::new(epoch(), 0.into()),
        through: ReplicationPosition::new(epoch(), 1.into()),
        transactions: vec![ReplicatedTransaction::SchemaStep {
            commit_id: 1.into(),
            migration_version: 1,
            statements: vec!["CREATE TABLE alarm_records(id INTEGER PRIMARY KEY)".into()],
        }],
        part: Some(
            TransactionPart::new(
                TransactionKey {
                    epoch: epoch(),
                    commit_id: 1.into(),
                },
                0,
                2,
                digest,
            )
            .expect("part"),
        ),
        summary: None,
    })
    .expect("piece");
    assert!(matches!(
        runtime
            .invoke(
                ObjectAddress::Copy(project()),
                WireCall::Copy(CopyCall::ApplyPiece {
                    piece: part.clone(),
                    sender: installation()
                })
            )
            .await
            .expect("stage"),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::PartStaged { .. }))
    ));
    server.shutdown().await.expect("stop");
    let server = LocalServer::start(settings()).await.expect("restart");
    server
        .runtime()
        .set_time(RegistryTime(now().0 + chrono::TimeDelta::days(2)))
        .await
        .expect("advance clock and fire overdue alarms");
    // Cleanup makes a repeated first part a fresh staging operation; its old
    // expiry must have been removed and replaced with the supplied time.
    server
        .runtime()
        .invoke(
            ObjectAddress::Copy(project()),
            WireCall::Copy(CopyCall::ApplyPiece {
                piece: part,
                sender: installation(),
            }),
        )
        .await
        .expect("restage");
    let alarms = server.runtime().pending_alarms().await;
    assert_eq!(alarms.len(), 1);
    assert!(alarms[0].1.0 > now().0 + chrono::TimeDelta::days(2));
    server.shutdown().await.expect("shutdown");
    jwks.shutdown().await.expect("JWKS shutdown");
}
```
