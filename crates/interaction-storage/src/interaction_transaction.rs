//! The interaction store's one transaction function. Every write, including the sync
//! library's `AtomicStore::transaction` entry, commits through it.
use crate::InteractionStore;
use sqlite_storage_maintenance::{CommitPoint, CommitSignal};
use sqlx::Connection;
use sqlx_turso::{Turso, TursoConnection};
use std::ops::{Deref, DerefMut};

/// How the transaction function opens its transaction.
#[derive(Clone, Copy, Debug)]
pub(crate) enum TransactionStart {
    /// `BEGIN IMMEDIATE`: takes the write lock first. Every mutating method uses it.
    Immediate,
    /// `BEGIN`: a consistent read snapshot.
    Deferred,
}

/// One transaction on the writer connection. Committing it signals subscribers only
/// when it changed rows.
pub(crate) struct InteractionTransaction<'connection> {
    transaction: sqlx::Transaction<'connection, Turso>,
    changes_at_begin: i64,
    commits: &'connection CommitSignal,
}

impl InteractionStore {
    /// Opens a transaction on the locked writer `connection`.
    pub(crate) async fn transaction<'connection>(
        &'connection self,
        connection: &'connection mut TursoConnection,
        start: TransactionStart,
    ) -> Result<InteractionTransaction<'connection>, sqlx::Error> {
        let mut transaction = match start {
            TransactionStart::Immediate => connection.begin_with("BEGIN IMMEDIATE").await?,
            TransactionStart::Deferred => connection.begin().await?,
        };
        let changes_at_begin = total_changes(&mut transaction).await?;
        Ok(InteractionTransaction {
            transaction,
            changes_at_begin,
            commits: &self.commits,
        })
    }

    /// A receiver that wakes after each committed transaction that changed rows.
    #[must_use]
    pub fn subscribe_commits(&self) -> tokio::sync::watch::Receiver<u64> {
        self.commits.subscribe()
    }

    /// The store's commit signal, for a test that holds a commit at a [`CommitPoint`].
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn commit_signal(&self) -> &CommitSignal {
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

impl InteractionTransaction<'_> {
    /// Commits, then bumps the commit signal if this transaction changed rows.
    pub(crate) async fn commit(mut self) -> Result<(), sqlx::Error> {
        let changed_rows = total_changes(&mut self.transaction).await? != self.changes_at_begin;
        let pending = self.commits.begin_commit(changed_rows);
        pending.checkpoint(CommitPoint::BeforeCommitInvoked).await;
        let committed = self.transaction.commit().await;
        if committed.is_ok() {
            pending.checkpoint(CommitPoint::AfterCommitReturned).await;
        }
        pending.settle(committed.is_ok());
        committed
    }

    /// A known rollback; it never signals.
    pub(crate) async fn rollback(self) -> Result<(), sqlx::Error> {
        self.transaction.rollback().await
    }
}

impl Deref for InteractionTransaction<'_> {
    type Target = TursoConnection;

    fn deref(&self) -> &TursoConnection {
        &self.transaction
    }
}

impl DerefMut for InteractionTransaction<'_> {
    fn deref_mut(&mut self) -> &mut TursoConnection {
        &mut self.transaction
    }
}

async fn total_changes(connection: &mut TursoConnection) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<Turso, i64>("SELECT total_changes()")
        .fetch_one(connection)
        .await
}

#[cfg(test)]
#[path = "interaction_transaction_test.rs"]
mod tests;
