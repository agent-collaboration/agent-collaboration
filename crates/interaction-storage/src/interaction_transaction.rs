//! The interaction store's one transaction function. Every write, including the sync
//! library's `AtomicStore::transaction` entry, commits through it.
use crate::InteractionStore;
use sqlite_storage_maintenance::SignaledTransaction;
use sqlx::Connection;
use sqlx_turso::{Turso, TursoConnection};
use std::{future::Future, pin::Pin};

/// One transaction on the writer connection; committing it signals subscribers only
/// when it changed rows.
pub(crate) type InteractionTransaction<'connection> = SignaledTransaction<'connection, Turso>;
pub(crate) use sqlite_storage_maintenance::TransactionStart;

impl InteractionStore {
    /// Opens a transaction on the locked writer `connection`.
    pub(crate) async fn transaction<'connection>(
        &'connection self,
        connection: &'connection mut TursoConnection,
        start: TransactionStart,
    ) -> Result<InteractionTransaction<'connection>, sqlx::Error> {
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
#[path = "interaction_transaction_test.rs"]
mod tests;
