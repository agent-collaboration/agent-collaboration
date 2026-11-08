//! A store's commit signal: a counter bumped after a transaction that changed rows commits.
//!
//! Workers subscribe before their first read and sleep until the counter moves. A read,
//! a transaction that changed nothing and a known rollback leave it alone. A commit
//! whose future is dropped while it runs bumps anyway, because its outcome is unknown;
//! the cost is one extra read after a write that in fact rolled back.
use sqlx::Database;
use std::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
};
use tokio::sync::watch;

/// The store-owned sender; each store's one transaction function settles through it.
#[derive(Clone, Debug)]
pub struct CommitSignal {
    commits: watch::Sender<u64>,
    #[cfg(feature = "test-support")]
    pause: std::sync::Arc<std::sync::Mutex<Option<test_support::ArmedPause>>>,
}

impl Default for CommitSignal {
    fn default() -> Self {
        Self::new()
    }
}

impl CommitSignal {
    #[must_use]
    pub fn new() -> Self {
        let (commits, _) = watch::channel(0);
        Self {
            commits,
            #[cfg(feature = "test-support")]
            pause: std::sync::Arc::default(),
        }
    }

    /// A receiver whose current value is already seen; only a later commit wakes it.
    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.commits.subscribe()
    }

    /// Starts settling one commit. `changed_rows` compares `total_changes()` read after
    /// `BEGIN` with the value read just before `COMMIT`.
    pub fn begin_commit(&self, changed_rows: bool) -> PendingCommit<'_> {
        PendingCommit {
            signal: self,
            armed: changed_rows,
        }
    }

    fn bump(&self) {
        self.commits
            .send_modify(|revision| *revision = revision.wrapping_add(1));
    }
}

/// How a store's transaction function opens its transaction.
#[derive(Clone, Copy, Debug)]
pub enum TransactionStart {
    /// `BEGIN IMMEDIATE`: takes the write lock first. Every mutating method uses it.
    Immediate,
    /// `BEGIN`: a consistent read snapshot.
    Deferred,
}

impl TransactionStart {
    /// The statement that opens this kind of transaction.
    #[must_use]
    pub const fn statement(self) -> &'static str {
        match self {
            Self::Immediate => "BEGIN IMMEDIATE",
            Self::Deferred => "BEGIN",
        }
    }
}

/// Reads the connection's `SELECT total_changes()`. Each store passes the concrete query
/// for its driver; SQLite and Turso both count every row an INSERT, UPDATE or DELETE
/// matched, even when it wrote the same value.
pub type ChangeCounter<TDatabase> = for<'connection> fn(
    &'connection mut <TDatabase as Database>::Connection,
) -> Pin<
    Box<dyn Future<Output = Result<i64, sqlx::Error>> + Send + 'connection>,
>;

/// The SQLite driver's [`ChangeCounter`].
pub fn sqlite_total_changes(
    connection: &mut sqlx::SqliteConnection,
) -> Pin<Box<dyn Future<Output = Result<i64, sqlx::Error>> + Send + '_>> {
    Box::pin(sqlx::query_scalar("SELECT total_changes()").fetch_one(connection))
}

/// One transaction whose commit signals subscribers only when it changed rows: it counts
/// changes after `BEGIN` and again just before `COMMIT`.
pub struct SignaledTransaction<'connection, TDatabase: Database> {
    transaction: sqlx::Transaction<'connection, TDatabase>,
    changes_at_begin: i64,
    count_changes: ChangeCounter<TDatabase>,
    commits: &'connection CommitSignal,
}

impl<'connection, TDatabase: Database> SignaledTransaction<'connection, TDatabase> {
    /// Takes a transaction the store just opened with [`TransactionStart::statement`];
    /// its commit settles through `commits`. The store opens it on its concrete
    /// connection, which keeps the future `Send` without higher-ranked bounds.
    pub async fn counting(
        mut transaction: sqlx::Transaction<'connection, TDatabase>,
        count_changes: ChangeCounter<TDatabase>,
        commits: &'connection CommitSignal,
    ) -> Result<Self, sqlx::Error> {
        let changes_at_begin = count_changes(&mut transaction).await?;
        Ok(Self {
            transaction,
            changes_at_begin,
            count_changes,
            commits,
        })
    }

    /// Commits, then bumps the commit signal if this transaction changed rows.
    pub async fn commit(mut self) -> Result<(), sqlx::Error> {
        let changed_rows =
            (self.count_changes)(&mut self.transaction).await? != self.changes_at_begin;
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
    pub async fn rollback(self) -> Result<(), sqlx::Error> {
        self.transaction.rollback().await
    }
}

impl<TDatabase: Database> Deref for SignaledTransaction<'_, TDatabase> {
    type Target = TDatabase::Connection;

    fn deref(&self) -> &TDatabase::Connection {
        &self.transaction
    }
}

impl<TDatabase: Database> DerefMut for SignaledTransaction<'_, TDatabase> {
    fn deref_mut(&mut self) -> &mut TDatabase::Connection {
        &mut self.transaction
    }
}

/// Where a store's transaction function pauses under `test-support`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommitPoint {
    /// After the pending signal is armed and before the backend `COMMIT` is invoked.
    BeforeCommitInvoked,
    /// After the backend `COMMIT` returned success and before the signal settles.
    AfterCommitReturned,
}

/// One commit being settled. Dropping it while armed bumps: the commit was cancelled
/// mid-way and may have landed.
#[must_use = "settle the commit with its result, or drop it only on cancellation"]
pub struct PendingCommit<'signal> {
    signal: &'signal CommitSignal,
    armed: bool,
}

impl PendingCommit<'_> {
    /// Bumps only when rows changed and the commit succeeded; a failed commit is a
    /// known rollback.
    pub fn settle(mut self, committed: bool) {
        if self.armed && committed {
            self.signal.bump();
        }
        self.armed = false;
    }

    /// Under `test-support`, a test can hold a commit that changed rows at `point`.
    #[cfg(feature = "test-support")]
    pub async fn checkpoint(&self, point: CommitPoint) {
        if self.armed {
            test_support::wait_at(self.signal, point).await;
        }
    }

    /// Outside `test-support` a commit never pauses.
    #[cfg(not(feature = "test-support"))]
    pub fn checkpoint(&self, _point: CommitPoint) -> std::future::Ready<()> {
        std::future::ready(())
    }
}

impl Drop for PendingCommit<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.signal.bump();
        }
    }
}

#[cfg(feature = "test-support")]
mod test_support {
    use super::{CommitPoint, CommitSignal};
    use tokio::sync::oneshot;

    #[derive(Debug)]
    pub(super) struct ArmedPause {
        point: CommitPoint,
        reached: oneshot::Sender<()>,
        release: oneshot::Receiver<()>,
    }

    /// The test's side of one armed pause.
    pub struct CommitPause {
        reached: oneshot::Receiver<()>,
        release: oneshot::Sender<()>,
    }

    impl CommitPause {
        /// Resolves once a commit is held at the armed point.
        pub async fn reached(&mut self) {
            let _ = (&mut self.reached).await;
        }

        /// Lets the held commit continue.
        pub fn release(self) {
            let _ = self.release.send(());
        }
    }

    impl CommitSignal {
        /// Holds the next commit that changed rows at `point` until released or cancelled.
        #[must_use]
        pub fn pause_next_commit_at(&self, point: CommitPoint) -> CommitPause {
            let (reached_sender, reached) = oneshot::channel();
            let (release, release_receiver) = oneshot::channel();
            if let Ok(mut pause) = self.pause.lock() {
                *pause = Some(ArmedPause {
                    point,
                    reached: reached_sender,
                    release: release_receiver,
                });
            }
            CommitPause { reached, release }
        }
    }

    pub(super) async fn wait_at(signal: &CommitSignal, point: CommitPoint) {
        let armed = match signal.pause.lock() {
            Ok(mut pause) if pause.as_ref().is_some_and(|armed| armed.point == point) => {
                pause.take()
            }
            _ => None,
        };
        if let Some(armed) = armed {
            let _ = armed.reached.send(());
            let _ = armed.release.await;
        }
    }
}

#[cfg(feature = "test-support")]
pub use test_support::CommitPause;

#[cfg(test)]
#[path = "commit_signal_test.rs"]
mod tests;
