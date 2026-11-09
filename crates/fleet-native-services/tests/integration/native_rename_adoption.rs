#![allow(clippy::panic_in_result_fn)]
use crate::consumer_fixture::{SampleSchema, TestResult};
use fleet_project_lease::LeasePolicy;
use fleet_project_writer::{
    OpenError, ReplicatedDatabase, ReplicatedSchema, WriteRefusal, WriterOptions,
};
use fleet_protocol::*;
use sqlx_core::{
    connection::{ConnectOptions, Connection},
    migrate::Migrator,
    query::query,
    row::Row,
};
use sqlx_turso::{Turso, TursoConnectOptions};
use std::{path::Path, time::Duration};

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
async fn sqlx_v1_adoption_still_refuses_an_invalid_key_before_capture_setup() -> TestResult {
    let root = tempfile::tempdir()?;
    let path = root.path().join("bad-prefix.db");
    pre_capture(&path, "not-a-uuid").await?;
    let result = ReplicatedDatabase::adopt(
        &path,
        SampleSchema::rename_bundle()?,
        PeerId::new(durable_steps::Uuid::from_u128(
            0x019eef00000070008000000000000002,
        ))?,
        LeasePolicy {
            lease_duration: Duration::from_secs(600),
            renew_interval: Duration::from_secs(30),
        },
        WriterOptions::default(),
    )
    .await;
    assert!(
        matches!(result, Err(OpenError::WriteRefused { refusal: WriteRefusal::InvalidKey { table } }) if table == "records")
    );
    let mut connection = TursoConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .connect()
        .await?;
    assert_eq!(
        query::<Turso>("SELECT id,value FROM records")
            .fetch_one(&mut connection)
            .await?
            .try_get::<String, _>("id")?,
        "not-a-uuid"
    );
    assert_eq!(
        query::<Turso>("SELECT version FROM _sqlx_migrations")
            .fetch_all(&mut connection)
            .await?
            .len(),
        1
    );
    assert!(query::<Turso>("SELECT name FROM sqlite_schema WHERE name IN ('_fleet_sync_writer','turso_cdc','tasks')").fetch_all(&mut connection).await?.is_empty());
    connection.close().await?;
    Ok(())
}
