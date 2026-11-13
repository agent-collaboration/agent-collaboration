use std::cell::RefCell;
use std::fmt::{self, Debug, Formatter};
use std::future::Future;
use std::rc::Rc;

use sqlx_core::error::Error;
use worker::send::SendFuture;

use crate::database::{DoConnection, DoTransaction};

impl DoConnection {
    /// Runs `callback` in a transaction: it commits if the callback returns
    /// `Ok`, and rolls back every write the callback made if it returns
    /// `Err`.
    ///
    /// A real interactive transaction, unlike a batch: the callback reads its
    /// own writes, and one statement can use what the one before it returned.
    /// It runs inside `Storage::transaction`, and the callback's value -- or
    /// its error, as it returned it -- is what this resolves to.
    ///
    /// ```no_run
    /// # async fn create(conn: &sqlx_durable_object::DoConnection, email: String) -> Result<i64, sqlx::Error> {
    /// let id = conn
    ///     .transaction(move |tx| async move {
    ///         let id: i64 = sqlx::query_scalar("INSERT INTO users (email) VALUES (?) RETURNING id")
    ///             .bind(email)
    ///             .fetch_one(&tx)
    ///             .await?;
    ///         sqlx::query("INSERT INTO posts (user_id, title) VALUES (?, 'hello')")
    ///             .bind(id)
    ///             .execute(&tx)
    ///             .await?;
    ///         Ok::<_, sqlx::Error>(id)
    ///     })
    ///     .await?;
    /// # Ok(id)
    /// # }
    /// ```
    ///
    /// # The callback owns what it uses
    ///
    /// `Storage::transaction` takes a `'static` callback, so it cannot borrow
    /// the connection or anything else from the caller: move what it needs
    /// in, and run its queries through the [`DoTransaction`] it is handed.
    /// That is also why this is a callback and not sqlx's `begin()`, which
    /// borrows the connection -- `begin()` still fails.
    ///
    /// # It holds the whole object
    ///
    /// The Durable Object delivers no other request until the transaction
    /// ends -- even while the callback awaits something that is not storage.
    /// That is what keeps other requests' writes out of it, and why it should
    /// be short: an awaited `fetch()` inside it stalls every caller of the
    /// object, and a request to the object itself would never be answered.
    ///
    /// # Errors
    ///
    /// The callback's own error, after rolling back; or, converted into `E`,
    /// an error from storage when the transaction could not commit.
    pub fn transaction<F, Fut, T, E>(
        &self,
        callback: F,
    ) -> impl Future<Output = Result<T, E>> + Send + '_
    where
        F: FnOnce(DoTransaction) -> Fut + 'static,
        Fut: Future<Output = Result<T, E>> + 'static,
        T: 'static,
        E: From<Error> + 'static,
    {
        let tx = DoTransaction {
            sql: self.sql.clone(),
        };

        // `Storage::transaction` and the callback's future hold JavaScript
        // handles, so neither is `Send`; `SendFuture` asserts it, soundly,
        // because a Durable Object runs on one thread.
        SendFuture::new(async move {
            // The callback reports through this rather than its return
            // value, which `worker` flattens to a string: the caller gets its
            // value, or its error as it returned it.
            let outcome = Rc::new(RefCell::new(None));
            let slot = Rc::clone(&outcome);

            let committed = self
                .storage
                .transaction(move |_| async move {
                    let result = callback(tx).await;
                    let failed = result.is_err();
                    *slot.borrow_mut() = Some(result);

                    if failed {
                        // Any error will do: rejecting is what rolls back.
                        Err(worker::Error::RustError("the callback failed".into()))
                    } else {
                        Ok(())
                    }
                })
                .await;

            let outcome = outcome.borrow_mut().take();
            match (committed, outcome) {
                (_, Some(Err(error))) => Err(error),
                (Ok(()), Some(Ok(value))) => Ok(value),
                (Err(error), _) => Err(E::from(Error::Protocol(format!(
                    "the Durable Object storage transaction failed: {error}"
                )))),
                (Ok(()), None) => Err(E::from(Error::Protocol(
                    "the Durable Object storage transaction never ran its callback".into(),
                ))),
            }
        })
    }
}

impl Debug for DoTransaction {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("DoTransaction").finish_non_exhaustive()
    }
}
