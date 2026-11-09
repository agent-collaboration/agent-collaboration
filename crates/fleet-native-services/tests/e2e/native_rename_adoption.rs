#![allow(clippy::panic_in_result_fn)]
use crate::consumer_fixture::{SampleSchema, TestResult, WriterCommand, WriterReply};
use crate::support::{writer_process::WriterProcess, *};
use fleet_project_writer::ReplicatedSchema;
use fleet_protocol::*;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    migrate::Migrator,
    query::query,
};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::path::Path;
const KEY: &str = "019eef00-0000-7000-8000-000000000001";

async fn pre_capture(path: &Path, key: &str) -> TestResult {
    let mut connection = TursoConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .connect()
        .await?;
    let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
    Migrator::with_migrations(SampleSchema::rename_prefix()?.migrations().to_vec())
        .run_direct(None, &mut *transaction, false)
        .await?;
    query::<Turso>("UPDATE records SET id=?,value='pre-capture preserved'")
        .bind(key)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    connection.close().await?;
    Ok(())
}

#[tokio::test]
async fn sqlx_v1_adoption_migrates_rename_and_seeds_then_follows_the_copy() -> TestResult {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("writer");
    tokio::fs::create_dir_all(&directory).await?;
    pre_capture(&directory.join("writer.db"), KEY).await?;
    let service = ServiceProcess::start(&root.path().join("service")).await?;
    let endpoint = format!("ws://127.0.0.1:{}/session", service.port);
    // adopt owns migration of the unapplied v2 after checking the exact v1 history.
    let mut writer = WriterProcess::start(&directory, &endpoint, "rename-adopt", None).await?;
    writer.command(WriterCommand::Register).await?;
    writer.command(WriterCommand::WaitReady).await?;
    let through = match writer.command(WriterCommand::Position).await? {
        WriterReply::Position(position) => position,
        _ => return Err("adopted writer position missing".into()),
    };
    writer.command(WriterCommand::WaitAcked { through }).await?;
    let mut reader = service
        .peer(5, SessionId::new(durable_steps::Uuid::now_v7())?)
        .await?;
    let rows = copy_rows(&mut reader, writer.database).await?;
    assert_eq!(
        rows.get("tasks"),
        Some(&vec![vec![
            SqlValue::Text(KEY.into()),
            SqlValue::Text("pre-capture preserved".into())
        ]])
    );
    assert!(!rows.contains_key("records"));
    let manifest = match writer.command(WriterCommand::SnapshotManifest).await? {
        WriterReply::SnapshotManifest(manifest) => manifest,
        _ => return Err("adopted writer manifest missing".into()),
    };
    assert_eq!(manifest.schema.version, 2);
    assert_eq!(manifest.tables.len(), 1);
    assert_eq!(manifest.tables[0].table.as_str(), "tasks");
    let next = match writer
        .command(WriterCommand::WriteRenamedRecord {
            value: "adopted follows".into(),
        })
        .await?
    {
        WriterReply::Position(position) => position,
        _ => return Err("adopted writer write refused".into()),
    };
    writer
        .command(WriterCommand::WaitAcked { through: next })
        .await?;
    let rows = copy_rows(&mut reader, writer.database).await?;
    assert_eq!(
        rows.get("tasks"),
        Some(&vec![vec![
            SqlValue::Text(KEY.into()),
            SqlValue::Text("adopted follows".into())
        ]])
    );
    assert!(!rows.contains_key("records"));
    writer.stop().await?;
    let mut control = service
        .connect("control", "Bearer consumer-control")
        .await?;
    service.stop(&mut control).await?;
    Ok(())
}
