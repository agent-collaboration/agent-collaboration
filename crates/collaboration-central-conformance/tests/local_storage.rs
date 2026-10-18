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
