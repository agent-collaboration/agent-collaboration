//! A missing central migration stamp keeps owner diagnostics available until recovery.
use super::{ConformanceResult, JwksFixtureServer, PROJECT, TokioServer, measured_files, settings};
use crate::{
    copy_owner_scenarios::{OwnerClient, bookkeeping, no_failures},
    creation_scenarios as creation,
};
use board_sync_protocol::BoardAnnotation;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer, owner_assertion};
use collaboration_central_tokio::{HealthFileSample, TokioCentralPlatform};
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::{
    CommitSequence, DatabaseId, Epoch, OperationAnswer, OperationRequest, PeerId, Piece,
    PieceContents, ReplicatedTransaction, ReplicationAnswer, ReplicationPosition,
    ReplicationRequest, RequestId, RowChange, SnapshotRead, SqlValue, TakeoverPreview,
};
use message_board::{BoardId, Description, ResourceName, WorkspaceId};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    query::query,
    row::Row,
};
use std::{future::Future, path::Path, time::Duration};

const SERVICE: &str = "550e8400-e29b-41d4-a716-446655440735";
const PEER: &str = "019f0000-0000-7000-8000-000000000735";
const RECORD_KEY: &str = "019f0000-0000-7000-8000-000000007735";

async fn serve<Scenario, Run, Output>(
    root: &Path,
    jwks: &str,
    scenario: Scenario,
) -> ConformanceResult<Output>
where
    Scenario: FnOnce(String, TokioCentralPlatform) -> Run,
    Run: Future<Output = ConformanceResult<Output>>,
{
    let server = TokioServer::start(settings(root, jwks)?).await?;
    let observed = scenario(
        server.base_url().to_owned(),
        server.conformance_platform().clone(),
    )
    .await;
    server.shutdown().await?;
    observed
}

async fn preview(base: &str) -> ConformanceResult<(StatusCode, Value)> {
    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?
        .post(format!(
            "{base}/admin/v1/projects/{PROJECT}/takeover/preview"
        ))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .send()
        .await?;
    Ok((response.status(), response.json().await?))
}

fn prefix() -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(2),
    ))
}

async fn create_project(base: &str) -> ConformanceResult<Value> {
    let client = ConformanceClient::new(base)?;
    let mut writer = enroll_admitted_peer(
        &client,
        SERVICE,
        PEER,
        "migration-reopen-writer",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let observed = async {
        let workspace = WorkspaceId::generate();
        let board = BoardId::generate();
        let database = DatabaseId::try_from(PROJECT.to_owned())?;
        for operation in [
            FleetOperation::WorkspaceCreate {
                name: ResourceName::try_from("migration reopen workspace".to_owned())?,
                description: Description::try_from("real owner recovery proof".to_owned())?,
            },
            FleetOperation::BoardCreate {
                board: board.clone(),
                name: ResourceName::try_from("migration reopen board".to_owned())?,
                description: Description::try_from("normal creation and replication".to_owned())?,
                board_kind: message_board::BoardKind::Area {},
            },
            creation::project(database, board)?,
        ] {
            creation::accepted(
                creation::metadata(&mut writer.socket, &workspace, operation).await?,
            )?;
        }
        let peer = PeerId::try_from(PEER.to_owned())?;
        creation::registered(
            &creation::register(
                &mut writer.socket,
                database,
                peer,
                RequestId::new(Uuid::now_v7())?,
            )
            .await?,
            peer,
        )?;
        let epoch = Epoch::new(1)?;
        let piece = Piece::<BoardAnnotation>::new(PieceContents {
            database,
            epoch,
            continues_from: ReplicationPosition::new(epoch, CommitSequence::new(0)),
            through: prefix()?,
            transactions: vec![
                ReplicatedTransaction::SchemaStep {
                    commit_id: CommitSequence::new(1),
                    migration_version: 1,
                    statements: vec![
                        "CREATE TABLE records(record_key TEXT NOT NULL PRIMARY KEY,id INTEGER NOT NULL,label TEXT NOT NULL)".into(),
                    ],
                },
                ReplicatedTransaction::Rows {
                    commit_id: CommitSequence::new(2),
                    rows: vec![RowChange::upsert(
                        "records",
                        vec![SqlValue::Text(RECORD_KEY.into())],
                        vec![SqlValue::Text(RECORD_KEY.into()), SqlValue::Integer(7), SqlValue::Text("keep".into())],
                    )?],
                },
            ],
            part: None,
            annotation: None,
        })?;
        let answer = creation::operation_answer(&mut writer.socket, OperationRequest::Replication(ReplicationRequest::Piece(piece))).await?;
        if !matches!(answer, OperationAnswer::Replication(ReplicationAnswer::Applied { through }) if through == prefix()?) {
            return Err(format!("migration fixture did not commit its normal schema and row Piece: {}", serde_json::to_string(&answer)?).into());
        }
        let (status, original) = preview(base).await?;
        let native: TakeoverPreview = serde_json::from_value(
            original
                .get("preview")
                .cloned()
                .ok_or("initial Preview omitted its native value")?,
        )?;
        if status != StatusCode::OK
            || native.holder != peer
            || native.fence != epoch
            || native.acked != prefix()?
        {
            return Err(format!(
                "initial Preview lost the original holder/fence/prefix: {original}"
            )
            .into());
        }
        Ok(original)
    }
    .await;
    let closed = writer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    closed?;
    observed
}

#[derive(Debug, PartialEq)]
struct MigrationStamp {
    row_id: String,
    corpus: String,
    version: String,
    checksum: String,
}

// The caller has stopped the owning server. Only central's own stamp is changed.
async fn remove_stamp(root: &Path) -> ConformanceResult<MigrationStamp> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{PROJECT}.db")))
        .connect()
        .await?;
    let result = async {
        let rows = query::<sqlx_turso::Turso>("SELECT row_id, corpus, version, checksum FROM central_schema_migrations WHERE corpus='fleet-sync-receiver' AND version='3'").fetch_all(&mut store).await?;
        let [row] = rows.as_slice() else { return Err("fixture did not have exactly one receiver version3 stamp".into()); };
        let stamp = MigrationStamp { row_id: row.try_get("row_id")?, corpus: row.try_get("corpus")?, version: row.try_get("version")?, checksum: row.try_get("checksum")? };
        query::<sqlx_turso::Turso>("DELETE FROM central_schema_migrations WHERE corpus=? AND version=? AND checksum=?").bind(&stamp.corpus).bind(&stamp.version).bind(&stamp.checksum).execute(&mut store).await?;
        Ok(stamp)
    }.await;
    store.close().await?;
    result
}

async fn restore_stamp(root: &Path, stamp: &MigrationStamp) -> ConformanceResult<()> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{PROJECT}.db")))
        .connect()
        .await?;
    let result = async {
        query::<sqlx_turso::Turso>("INSERT INTO central_schema_migrations(row_id,corpus,version,checksum) VALUES(?,?,?,?)").bind(&stamp.row_id).bind(&stamp.corpus).bind(&stamp.version).bind(&stamp.checksum).execute(&mut store).await?;
        let row = query::<sqlx_turso::Turso>("SELECT row_id,corpus,version,checksum FROM central_schema_migrations WHERE corpus=? AND version=?").bind(&stamp.corpus).bind(&stamp.version).fetch_one(&mut store).await?;
        let restored = MigrationStamp { row_id: row.try_get("row_id")?, corpus: row.try_get("corpus")?, version: row.try_get("version")?, checksum: row.try_get("checksum")? };
        if restored != *stamp { return Err("fixture did not restore the exact saved migration stamp".into()); }
        Ok(())
    }.await;
    store.close().await?;
    result
}

async fn stored_failure(root: &Path) -> ConformanceResult<Value> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{PROJECT}.db")))
        .read_only(true)
        .connect()
        .await?;
    let result = async {
        let rows = query::<sqlx_turso::Turso>(
            "SELECT corpus,version,reason,at FROM central_schema_migration_failures",
        )
        .fetch_all(&mut store)
        .await?;
        let [row] = rows.as_slice() else {
            return Err("failed bootstrap did not persist exactly one central diagnostic".into());
        };
        let corpus: String = row.try_get("corpus")?;
        let version: String = row.try_get("version")?;
        let reason: String = row.try_get("reason")?;
        let at: String = row.try_get("at")?;
        if corpus != "fleet-sync-receiver" || version != "3" || !reason.contains("already exists") {
            return Err(
                format!("unexpected actual migration cause: {corpus}/{version}: {reason}").into(),
            );
        }
        let at = chrono::DateTime::from_timestamp_millis(at.parse()?)
            .ok_or("stored migration failure time is outside the fixture range")?;
        Ok(json!({"kind":"known","value":[{"corpus":corpus,"version":3,"reason":reason,"at":at}]}))
    }
    .await;
    store.close().await?;
    result
}

async fn sampled_storage_bytes(
    platform: &TokioCentralPlatform,
    root: &Path,
) -> ConformanceResult<u64> {
    let sample = platform
        .conformance_health_sample(DatabaseId::try_from(PROJECT.to_owned())?)
        .await
        .ok_or("Health did not retain its own physical sample")?;
    let expected = [
        root.join(format!("copy-{PROJECT}.db")),
        root.join(format!("copy-{PROJECT}.db-wal")),
    ];
    let targets: Vec<_> = sample
        .iter()
        .map(|part| match part {
            HealthFileSample::Present { target, .. } | HealthFileSample::Missing { target } => {
                target.clone()
            }
        })
        .collect();
    if targets != expected {
        return Err("Health sampled files outside its actual target db and WAL".into());
    }
    sample.iter().try_fold(0_u64, |total, part| match part {
        HealthFileSample::Present { length, .. } => total
            .checked_add(*length)
            .ok_or_else(|| "independent sample sum overflow".into()),
        HealthFileSample::Missing { target } if target == &expected[1] => Ok(total),
        HealthFileSample::Missing { .. } => {
            Err("Health did not stat an existing target database".into())
        }
    })
}

// A real committed write after Health forces its later filesystem sample to
// differ. Restore the exact diagnostic value; only physical allocation changes.
async fn grow_files_after_health(root: &Path) -> ConformanceResult<()> {
    let mut store = sqlx_turso::TursoConnectOptions::new()
        .filename(root.join(format!("copy-{PROJECT}.db")))
        .connect()
        .await?;
    let result = async {
        let row =
            query::<sqlx_turso::Turso>("SELECT reason FROM central_schema_migration_failures")
                .fetch_one(&mut store)
                .await?;
        let original: String = row.try_get("reason")?;
        let larger = format!("{original}{}", " measurement-after-health".repeat(65536));
        let changed =
            query::<sqlx_turso::Turso>("UPDATE central_schema_migration_failures SET reason=?")
                .bind(larger)
                .execute(&mut store)
                .await?;
        if changed.rows_affected() != 1 {
            return Err("fixture growth did not update its one diagnostic".into());
        }
        let restored =
            query::<sqlx_turso::Turso>("UPDATE central_schema_migration_failures SET reason=?")
                .bind(original)
                .execute(&mut store)
                .await?;
        if restored.rows_affected() != 1 {
            return Err("fixture did not restore its exact diagnostic".into());
        }
        Ok(())
    }
    .await;
    store.close().await?;
    result
}

async fn read_recovered_row(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let mut reader = enroll_admitted_peer(
        &client,
        "550e8400-e29b-41d4-a716-446655440736",
        "019f0000-0000-7000-8000-000000000736",
        "migration-reopen-reader",
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let observed = async {
        match creation::operation_answer(
            &mut reader.socket,
            OperationRequest::Replication(ReplicationRequest::SnapshotPage {
                database: DatabaseId::try_from(PROJECT.to_owned())?,
                cursor: None,
            }),
        )
        .await?
        {
            OperationAnswer::Replication(ReplicationAnswer::SnapshotPage(SnapshotRead::Page(
                page,
            ))) if page.database.to_string() == PROJECT
                && page.position == prefix()?
                && page.schema.version == 1
                && page.table == "records"
                && page.rows
                    == vec![vec![
                        SqlValue::Text(RECORD_KEY.into()),
                        SqlValue::Integer(7),
                        SqlValue::Text("keep".into()),
                    ]]
                && page.next.is_none() =>
            {
                Ok(())
            }
            other => Err(format!(
                "reopened Copy lost the original schema, prefix or row: {}",
                serde_json::to_string(&other)?
            )
            .into()),
        }
    }
    .await;
    let closed = reader
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    closed?;
    observed
}

#[tokio::test]
async fn pending_migration_keeps_health_readable_and_preview_recovers_with_original_rows()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let canonical_root = root.path().canonicalize()?;
    let root_path = canonical_root.as_path();
    let proof = async {
        // Arrange a real registered Copy, then interrupt central-owned bookkeeping at rest.
        let original = serve(root_path, &jwks.url, |base, _platform| async move { create_project(&base).await }).await?;
        let stamp = remove_stamp(root_path).await?;
        // Act through the ordinary prelude and owner HTTP; Health remains tolerant.
        let failed = serve(root_path, &jwks.url, |base, platform| async move {
            pending_session_refusals(&base).await?;
            let owner = OwnerClient::new(&base)?;
            let pending = json!({"kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"version":3}}});
            let (status, body) = owner.repair(PROJECT).await?;
            if status != StatusCode::SERVICE_UNAVAILABLE || body != pending { return Err(format!("reopened migration did not retain typed MigrationPending3: {status}: {body}").into()); }
            let (status, body) = preview(&base).await?;
            if status != StatusCode::SERVICE_UNAVAILABLE || body != json!({"kind":"objectUnavailable"}) { return Err(format!("Preview served despite pending migration: {status}: {body}").into()); }
            let database = DatabaseId::try_from(PROJECT.to_owned())?;
            if platform.conformance_health_sample(database).await.is_some() { return Err("fixture had another Health sample before its deciding call".into()); }
            let health = owner.health(PROJECT).await?;
            bookkeeping(&health, Some(2))?;
            let measured_at_health = sampled_storage_bytes(&platform, root_path).await?;
            grow_files_after_health(root_path).await?;
            let later = measured_files(root_path)?;
            if later == measured_at_health { return Err("real post-response SQL write did not force a distinct later file size".into()); }
            if health.pointer("/value/storageBytes").and_then(Value::as_u64) != Some(measured_at_health) { return Err(format!("Health bytes differ from its actual same-call sample: reported={:?}, sample={measured_at_health}, later={later}",health.pointer("/value/storageBytes")).into()); }
            println!("G47_HEALTH_SAMPLE reported={measured_at_health} sampled={measured_at_health} later={later}");
            Ok(health)
        }).await?;
        if failed.pointer("/value/migrationFailures") != Some(&stored_failure(root_path).await?) { return Err("Health hid or changed the exact stored pending migration diagnostic".into()); }
        println!("OWNER_MIGRATION_REOPEN_CASE pending_health_exact_diagnostic_bytes_and_preview_refusal passed");
        // Assert recovery after exact restoration, including the normal snapshot contents.
        restore_stamp(root_path, &stamp).await?;
        serve(root_path, &jwks.url, |base, _platform| async move {
            let (status, recovered) = preview(&base).await?;
            if status != StatusCode::OK { return Err(format!("Preview did not recover after exact stamp restore: {status}: {recovered}").into()); }
            let recovered_native: TakeoverPreview = serde_json::from_value(recovered.get("preview").cloned().ok_or("recovered Preview omitted its native value")?)?;
            let original_native: TakeoverPreview = serde_json::from_value(original.get("preview").cloned().ok_or("original Preview omitted its native value")?)?;
            // A new Preview has a new operation token and current observations.
            // The retained recovery oracle is the original holder, fence and prefix.
            if recovered_native.fence != original_native.fence || recovered_native.acked != original_native.acked || recovered_native.holder != original_native.holder || recovered.get("holderInstallation") != Some(&json!(SERVICE)) { return Err(format!("Preview did not recover its original native fence/prefix after exact stamp restore: {status}: {recovered}").into()); }
            let health = OwnerClient::new(&base)?.health(PROJECT).await?;
            bookkeeping(&health, Some(3))?;
            no_failures(&health)?;
            read_recovered_row(&base).await?;
            println!("OWNER_MIGRATION_REOPEN_CASE restored_preview_and_original_snapshot_row passed");
            Ok(())
        }).await
    }.await;
    jwks.shutdown().await?;
    proof
}

// The held project's lease is already enrolled; reconnect without relabeling it.
async fn pending_session_refusals(base: &str) -> ConformanceResult<()> {
    use board_sync_protocol::BoardConsumerProtocol;
    use fleet_sync_protocol::{
        ClientFrame, CopyUnavailableReason, DatabaseStatus, SchemaSupport, ServerFrame, SessionId,
    };
    use futures_util::SinkExt;
    use reqwest_websocket::{Message, RequestBuilderExt};
    let mut socket = reqwest::Client::new()
        .get(format!("{base}/v1/session"))
        .bearer_auth("migration-reopen-writer")
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::machine_assertion("migration-reopen-writer")?,
        )
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    let result = async {
        let session = SessionId::new(Uuid::now_v7())?;
        socket.send(Message::Binary(serde_json::to_vec(&ClientFrame::<BoardConsumerProtocol>::Hello {session_id:session,schema_support:SchemaSupport {max_version:1}})?.into())).await?;
        let welcome = {
            use futures_util::StreamExt;
            match tokio::time::timeout(Duration::from_secs(5),socket.next()).await?.ok_or("pending Welcome missing")?? {
                Message::Binary(bytes)=>serde_json::from_slice(&bytes)?,
                Message::Text(text)=>serde_json::from_str(&text)?,
                _=>return Err("pending Welcome was not data".into()),
            }
        };
        let frame:ServerFrame<BoardConsumerProtocol> = serde_json::from_value(welcome)?;
        assert!(matches!(frame,ServerFrame::Welcome {session_id,databases} if session_id==session && databases.iter().any(|database|matches!(database,DatabaseStatus::Unavailable {database,lease:Some(_),reason:CopyUnavailableReason::MigrationPending {version:3}} if database.to_string()==PROJECT))),"Welcome lost pending Copy reason");
        let control = DatabaseId::new(Uuid::now_v7())?;
        let copies=crate::copy_wait_scenarios::CopyClient::new(base)?;
        let created=copies.invoke(json!({"kind":"create","value":{"database":control,"epoch":1,"holder":PEER,"origin":{"kind":"empty"}}})).await?;
        copies.completed(created,crate::copy_wait_scenarios::expected("created",fleet_sync_protocol::CreateCopyAnswer::CopyCreated)).await?;
        let control_piece = |sequence,statement: &str| -> ConformanceResult<Piece<BoardAnnotation>> {
            Ok(Piece::new(PieceContents {database:control,epoch:Epoch::new(1)?,continues_from:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(sequence-1)),through:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(sequence)),transactions:vec![ReplicatedTransaction::SchemaStep {commit_id:CommitSequence::new(sequence),migration_version:sequence,statements:vec![statement.into()]}],part:None,annotation:None})?)
        };
        let first = creation::operation_answer(&mut socket,OperationRequest::Replication(ReplicationRequest::Piece(control_piece(1,"CREATE TABLE control_rows(record_key TEXT NOT NULL PRIMARY KEY)")?))).await?;
        assert!(matches!(first,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through.sequence.value()==1));
        let database = DatabaseId::try_from(PROJECT.to_owned())?;
        let piece = Piece::<BoardAnnotation>::new(PieceContents {
            database,epoch:Epoch::new(1)?,continues_from:prefix()?,through:ReplicationPosition::new(Epoch::new(1)?,CommitSequence::new(3)),
            transactions:vec![ReplicatedTransaction::Rows {commit_id:CommitSequence::new(3),rows:vec![RowChange::upsert("records",vec![SqlValue::Text(RECORD_KEY.into())],vec![SqlValue::Text(RECORD_KEY.into()),SqlValue::Integer(7),SqlValue::Text("must roll back".into())])?]}],part:None,annotation:None,
        })?;
        for request in [ReplicationRequest::CopyStatus {database},ReplicationRequest::SnapshotPage {database,cursor:None},ReplicationRequest::Piece(piece)] {
            let answer = creation::operation_answer(&mut socket,OperationRequest::Replication(request)).await?;
            assert!(matches!(answer,OperationAnswer::Replication(ReplicationAnswer::CopyUnavailable {reason:CopyUnavailableReason::MigrationPending {version:3}})),"session lost exact migration refusal: {}",serde_json::to_string(&answer)?);
        }
        let next = creation::operation_answer(&mut socket,OperationRequest::Replication(ReplicationRequest::Piece(control_piece(2,"ALTER TABLE control_rows ADD COLUMN label TEXT")?))).await?;
        assert!(matches!(next,OperationAnswer::Replication(ReplicationAnswer::Applied {through}) if through.sequence.value()==2),"healthy Copy stopped after another Copy's refusal");
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let _ = socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result
}
