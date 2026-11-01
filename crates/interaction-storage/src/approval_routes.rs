//! Approval route payloads and their last-use retention timestamps.
use crate::{InteractionStore, StorageError};
use sqlx::Connection;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredApprovalRoute {
    pub thread_id: String,
    pub route_json: String,
}

impl StoredApprovalRoute {
    fn validate(&self) -> Result<(), StorageError> {
        serde_json::from_str::<serde_json::Value>(&self.route_json)
            .map_err(|_| StorageError::InvalidRecord)?;
        Ok(())
    }
}

impl InteractionStore {
    /// Imports the legacy file's full route set in one transaction.
    pub async fn import_approval_routes(
        &self,
        routes: &[StoredApprovalRoute],
        imported_at_ms: i64,
    ) -> Result<(), StorageError> {
        for route in routes {
            route.validate()?;
        }

        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let imported = async {
            for route in routes {
                sqlx::query::<sqlx_turso::Turso>(
                    "INSERT OR IGNORE INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,?,?)",
                )
                .bind(&route.thread_id)
                .bind(&route.route_json)
                .bind(imported_at_ms)
                .execute(&mut *transaction)
                .await?;
            }
            Ok::<_, StorageError>(())
        }
        .await;
        if let Err(error) = imported {
            transaction.rollback().await?;
            return Err(error);
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Replaces a route and records registration as its latest use.
    pub async fn upsert_approval_route(
        &self,
        route: &StoredApprovalRoute,
        used_at_ms: i64,
    ) -> Result<(), StorageError> {
        route.validate()?;
        sqlx::query::<sqlx_turso::Turso>(
            "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,?,?) ON CONFLICT(thread_id) DO UPDATE SET route_json=excluded.route_json,last_used_at_ms=excluded.last_used_at_ms",
        )
        .bind(&route.thread_id)
        .bind(&route.route_json)
        .bind(used_at_ms)
        .execute(&mut *self.writer.lock().await)
        .await?;
        Ok(())
    }

    /// Reads one route and updates its last-use time in the same transaction.
    pub async fn approval_route_json(
        &self,
        thread_id: &str,
        used_at_ms: i64,
    ) -> Result<Option<String>, StorageError> {
        let mut connection = self.writer.lock().await;
        let mut transaction = connection.begin_with("BEGIN IMMEDIATE").await?;
        let accessed = async {
            let route_json = sqlx::query_scalar::<sqlx_turso::Turso, String>(
                "SELECT route_json FROM approval_routes WHERE thread_id=?",
            )
            .bind(thread_id)
            .fetch_optional(&mut *transaction)
            .await?;
            if route_json.is_some() {
                sqlx::query::<sqlx_turso::Turso>(
                    "UPDATE approval_routes SET last_used_at_ms=? WHERE thread_id=?",
                )
                .bind(used_at_ms)
                .bind(thread_id)
                .execute(&mut *transaction)
                .await?;
            }
            Ok::<_, sqlx::Error>(route_json)
        }
        .await;
        let route_json = match accessed {
            Ok(route_json) => route_json,
            Err(error) => {
                transaction.rollback().await?;
                return Err(StorageError::Database(error));
            }
        };
        transaction.commit().await?;
        Ok(route_json)
    }
}

pub(crate) async fn prune_expired(
    connection: &mut sqlx_turso::TursoConnection,
    cutoff_ms: i64,
    batch_size: i64,
) -> Result<u64, StorageError> {
    let deleted = sqlx::query::<sqlx_turso::Turso>(
        "DELETE FROM approval_routes WHERE thread_id IN (SELECT thread_id FROM approval_routes WHERE last_used_at_ms<? ORDER BY last_used_at_ms,thread_id LIMIT ?)",
    )
    .bind(cutoff_ms)
    .bind(batch_size)
    .execute(connection)
    .await?
    .rows_affected();
    Ok(deleted)
}
