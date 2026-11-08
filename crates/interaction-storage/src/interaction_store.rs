//! The serialized physical owner; library scopes borrow its transaction connection.
use crate::{StorageError, approval_routes::StoredApprovalRoute};
use sqlite_storage_maintenance::{SignaledTransaction, TransactionStart};
use sqlx::Connection;
use sqlx_turso::{Turso, TursoConnection};
use std::{future::Future, path::Path, pin::Pin};
use tokio::sync::Mutex;

pub struct InteractionStore {
    pub(crate) writer: Mutex<TursoConnection>,
    pub(crate) commits: sqlite_storage_maintenance::CommitSignal,
}

impl InteractionStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let (connection, _) =
            crate::interaction_history::store::interaction_history_database::initialize_history(
                path.as_ref(),
            )
            .await?;
        Ok(Self {
            writer: Mutex::new(connection),
            commits: sqlite_storage_maintenance::CommitSignal::new(),
        })
    }

    pub async fn close(self) -> Result<(), StorageError> {
        self.writer.into_inner().close().await?;
        Ok(())
    }

    /// Forgets the central status stream before a new enrollment attaches.
    pub async fn clear_central_mail_status_cursor(&self) -> Result<(), StorageError> {
        self.execute_as_transaction(sqlx::query::<sqlx_turso::Turso>(
            "DELETE FROM _fleet_sync_outbox_status",
        ))
        .await?;
        Ok(())
    }

    /// Imports the legacy file's full route set in one transaction.
    pub async fn import_approval_routes(
        &self,
        routes: &[StoredApprovalRoute],
        imported_at_ms: i64,
    ) -> Result<(), StorageError> {
        for route in routes {
            route.validate().map_err(|_| StorageError::InvalidRecord)?;
        }

        let mut connection = self.writer.lock().await;
        let mut transaction = self
            .transaction(
                &mut connection,
                sqlite_storage_maintenance::TransactionStart::Immediate,
            )
            .await?;
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
        route.validate().map_err(|_| StorageError::InvalidRecord)?;
        self.execute_as_transaction(sqlx::query::<sqlx_turso::Turso>(
            "INSERT INTO approval_routes(thread_id,route_json,last_used_at_ms) VALUES (?,?,?) ON CONFLICT(thread_id) DO UPDATE SET route_json=excluded.route_json,last_used_at_ms=excluded.last_used_at_ms",
        )
        .bind(&route.thread_id)
        .bind(&route.route_json)
        .bind(used_at_ms))
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
        let mut transaction = self
            .transaction(
                &mut connection,
                sqlite_storage_maintenance::TransactionStart::Immediate,
            )
            .await?;
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

/// The store's one transaction function. Every write, including the sync library's
/// `AtomicStore::transaction` entry, commits through it and signals subscribers only when
/// it changed rows.
impl InteractionStore {
    /// Opens a transaction on the locked writer `connection`.
    pub(crate) async fn transaction<'connection>(
        &'connection self,
        connection: &'connection mut TursoConnection,
        start: TransactionStart,
    ) -> Result<SignaledTransaction<'connection, Turso>, sqlx::Error> {
        let transaction = connection.begin_with(start.statement()).await?;
        SignaledTransaction::counting(transaction, turso_total_changes, &self.commits).await
    }

    /// A receiver that wakes after each committed transaction that changed rows.
    #[must_use]
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.commits.subscribe()
    }

    /// The store's commit signal, for a test that holds a commit at a [`CommitPoint`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn commit_signal(&self) -> &sqlite_storage_maintenance::CommitSignal {
        &self.commits
    }

    /// Runs one write statement as a one-statement transaction.
    pub(crate) async fn execute_as_transaction<'q, TArguments>(
        &self,
        query: sqlx::query::Query<'q, Turso, TArguments>,
    ) -> Result<sqlx_turso::TursoQueryResult, sqlx::Error>
    where
        TArguments: sqlx::IntoArguments<Turso> + 'q,
    {
        let mut connection = self.writer.lock().await;
        let mut transaction = self
            .transaction(&mut connection, TransactionStart::Immediate)
            .await?;
        match query.execute(&mut *transaction).await {
            Ok(result) => {
                transaction.commit().await?;
                Ok(result)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }
}

/// The Turso driver's change counter for [`SignaledTransaction`].
fn turso_total_changes(
    connection: &mut TursoConnection,
) -> Pin<Box<dyn Future<Output = Result<i64, sqlx::Error>> + Send + '_>> {
    Box::pin(sqlx::query_scalar::<Turso, i64>("SELECT total_changes()").fetch_one(connection))
}

#[cfg(test)]
#[path = "interaction_store_reopen_test.rs"]
mod reopen_tests;

#[cfg(test)]
#[path = "interaction_store_transaction_test.rs"]
mod transaction_tests;
