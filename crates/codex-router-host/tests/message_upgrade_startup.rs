#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::large_futures,
    reason = "lint pass follows the crate split"
)]
//! A pending message upgrade stops actual Host composition before any listener is published.
use automation_storage::StorageError;
use codex_router_host::{CollaborationRuntime, CollaborationRuntimeInputs};
use sqlx::{
    Connection,
    migrate::Migrator,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous},
};
use std::{os::unix::fs::PermissionsExt, path::Path};

type TestError = Box<dyn std::error::Error + Send + Sync>;
static MIGRATOR: Migrator = sqlx::migrate!("../automation-storage/migrations");

#[derive(Clone, Copy)]
enum SourceRow {
    ShippedPush,
    PreReleaseMail,
}

async fn connect(path: &Path) -> Result<sqlx::SqliteConnection, sqlx::Error> {
    sqlx::SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Delete)
            .synchronous(SqliteSynchronous::Full),
    )
    .await
}

async fn assert_startup_refuses(source: SourceRow) -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    let path = root.path().join("automation.sqlite");
    let mut connection = connect(&path).await?;
    MIGRATOR
        .run_direct(Some(20_261_019_000_200), &mut connection, false)
        .await?;
    let (insert, read) = match source {
        SourceRow::ShippedPush => (
            "INSERT INTO router_pushes(push_id,kind,origin_kind,target_service_id,target_endpoint_id,target_session_id,header_facts_json,body,delivery_state,created_at) VALUES ('source-push','directMessage','owner','fixture-service','fixture-endpoint','fixture-session','{}','source bytes stay','pending','2026-10-01T00:00:00Z')",
            "SELECT body FROM router_pushes WHERE push_id='source-push'",
        ),
        SourceRow::PreReleaseMail => (
            "INSERT INTO mail_items(item_id,digest,direction,status_json,status_version,payload_bytes,created_at) VALUES ('source-mail','opaque-digest','local','source bytes stay',1,17,'2026-10-01T00:00:00Z')",
            "SELECT status_json FROM mail_items WHERE item_id='source-mail'",
        ),
    };
    sqlx::query(insert).execute(&mut connection).await?;
    let ledger: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut connection)
            .await?;
    connection.close().await?;

    let error = match CollaborationRuntime::start(CollaborationRuntimeInputs {
        board_session: None,
        directory: root.path().to_owned(),
        codex_home: root.path().to_owned(),
        backend_socket: root.path().join("backend.sock"),
        mcp_bind: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
        native_schema: None,
        peer_registry_directory: None,
        remote_control_server_name: None,
        owner_human_id: None,
    })
    .await
    {
        Ok(runtime) => {
            runtime.shutdown().await?;
            return Err("Host started with uncopied message state".into());
        }
        Err(error) => error,
    };
    let typed = error
        .get_ref()
        .and_then(|error| error.downcast_ref::<StorageError>())
        .ok_or("startup lost its typed upgrade refusal")?;
    assert!(
        matches!(
            (source, typed),
            (
                SourceRow::ShippedPush,
                StorageError::MessageUpgradeRequired { .. }
            ) | (
                SourceRow::PreReleaseMail,
                StorageError::PreReleaseMailPresent { rows: 1 }
            )
        ),
        "{error:?}"
    );
    assert!(error.to_string().contains("codex-router-board-upgrade"));
    assert!(
        !root.path().join("service.json").exists(),
        "startup published a partial communication manifest"
    );
    let mut connection = connect(&path).await?;
    let bytes: String = sqlx::query_scalar(read).fetch_one(&mut connection).await?;
    assert_eq!(bytes, "source bytes stay");
    let after: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut connection)
            .await?;
    assert_eq!(after, ledger);
    connection.close().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn startup_refuses_uncopied_messages_before_publishing_the_host() -> Result<(), TestError> {
    assert_startup_refuses(SourceRow::ShippedPush).await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn startup_refuses_pre_release_mail_without_discarding_its_original_bytes()
-> Result<(), TestError> {
    assert_startup_refuses(SourceRow::PreReleaseMail).await
}
