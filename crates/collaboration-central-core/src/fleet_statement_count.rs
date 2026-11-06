//! Count execution at the real SQLx boundary, independently of tracing filters and caches.
//! Like QueryLogger, count SQLx requests, excluding preparation and internal schema probes.
#[path = "fleet_statement_count_test.rs"]
mod tests;

use durable_steps::storage::TransactionScope;
use futures_util::{FutureExt, StreamExt, TryStreamExt, future::BoxFuture, stream::BoxStream};
use sqlx_core::{
    Either, Error,
    database::Database,
    describe::Describe,
    executor::{Execute, Executor},
    sql_str::SqlStr,
};

pub(super) struct StatementCountScope<'scope, Scope> {
    scope: &'scope mut Scope,
    statement_count: usize,
}

impl<'scope, Scope> StatementCountScope<'scope, Scope> {
    pub(super) fn new(scope: &'scope mut Scope) -> Self {
        Self {
            scope,
            statement_count: 0,
        }
    }

    pub(super) fn statement_count(&self) -> usize {
        self.statement_count
    }
}

impl<Scope: TransactionScope> TransactionScope for StatementCountScope<'_, Scope> {
    type Database = Scope::Database;
    type Executor<'query>
        = StatementCountExecutor<'query, Scope::Executor<'query>>
    where
        Self: 'query;

    fn executor(&mut self) -> Self::Executor<'_> {
        StatementCountExecutor {
            executor: self.scope.executor(),
            statement_count: &mut self.statement_count,
        }
    }
}

#[derive(Debug)]
pub(super) struct StatementCountExecutor<'count, Inner> {
    executor: Inner,
    statement_count: &'count mut usize,
}

impl<'connection, Inner: Executor<'connection>> Executor<'connection>
    for StatementCountExecutor<'connection, Inner>
{
    type Database = Inner::Database;

    fn fetch_many<'execute, 'query: 'execute, Query>(
        self,
        query: Query,
    ) -> BoxStream<
        'execute,
        Result<
            Either<<Self::Database as Database>::QueryResult, <Self::Database as Database>::Row>,
            Error,
        >,
    >
    where
        'connection: 'execute,
        Query: 'query + Execute<'query, Self::Database>,
    {
        // Count on first poll, so merely constructing a query or stream is free.
        let stream = self.executor.fetch_many(query);
        futures_util::stream::once(async move {
            *self.statement_count += 1;
            Ok::<_, Error>(stream)
        })
        .try_flatten()
        .boxed()
    }

    fn fetch_optional<'execute, 'query: 'execute, Query>(
        self,
        query: Query,
    ) -> BoxFuture<'execute, Result<Option<<Self::Database as Database>::Row>, Error>>
    where
        'connection: 'execute,
        Query: 'query + Execute<'query, Self::Database>,
    {
        let future = self.executor.fetch_optional(query);
        async move {
            *self.statement_count += 1;
            future.await
        }
        .boxed()
    }

    fn prepare_with<'execute>(
        self,
        sql: SqlStr,
        parameters: &'execute [<Self::Database as Database>::TypeInfo],
    ) -> BoxFuture<'execute, Result<<Self::Database as Database>::Statement, Error>>
    where
        'connection: 'execute,
    {
        self.executor.prepare_with(sql, parameters)
    }

    fn describe<'execute>(
        self,
        sql: SqlStr,
    ) -> BoxFuture<'execute, Result<Describe<Self::Database>, Error>>
    where
        'connection: 'execute,
    {
        self.executor.describe(sql)
    }
}
