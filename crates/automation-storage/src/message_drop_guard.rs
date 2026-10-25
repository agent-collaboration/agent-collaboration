//! The migration ledger authorizes the drop; table existence never proves a copy.
use crate::{StorageError, schema_initialization::InitializationPurpose};
use sqlx::SqliteConnection;

pub(crate) const MESSAGE_DROP_VERSION: i64 = 20_261_021_000_100;
pub(crate) const PRE_MESSAGE_DROP_VERSION: i64 = 20_261_019_000_200;

pub(crate) async fn check(
    connection: &mut SqliteConnection,
    purpose: InitializationPurpose,
) -> Result<(), StorageError> {
    let applied: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM _sqlx_migrations WHERE version=?)")
            .bind(MESSAGE_DROP_VERSION)
            .fetch_one(&mut *connection)
            .await?;
    if applied {
        return Ok(());
    }
    let mail_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM mail_items")
        .fetch_one(&mut *connection)
        .await?;
    if mail_rows != 0 {
        return Err(StorageError::PreReleaseMailPresent {
            rows: u64::try_from(mail_rows).map_err(|_| StorageError::InvalidRecord)?,
        });
    }
    if matches!(purpose, InitializationPurpose::VerifiedMessageCopy) {
        return Ok(());
    }
    let mut tables = Vec::new();
    for (table, query) in [
        (
            "router_pushes",
            "SELECT EXISTS(SELECT 1 FROM router_pushes)",
        ),
        (
            "subscription_delivery_progress",
            "SELECT EXISTS(SELECT 1 FROM subscription_delivery_progress)",
        ),
        (
            "board_intent_progress",
            "SELECT EXISTS(SELECT 1 FROM board_intent_progress)",
        ),
        (
            "board_intent_scan_cursors",
            "SELECT EXISTS(SELECT 1 FROM board_intent_scan_cursors)",
        ),
    ] {
        if sqlx::query_scalar::<_, bool>(query)
            .fetch_one(&mut *connection)
            .await?
        {
            tables.push(table);
        }
    }
    if tables.is_empty() {
        Ok(())
    } else {
        Err(StorageError::MessageUpgradeRequired { tables })
    }
}
