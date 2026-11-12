//! `Do`, the types SQLx's `Database` trait names for each role (arguments,
//! column, connection, row, statement, transaction manager, value), and every SQLx
//! trait impl that names `Do` back. They point at each other by the trait's design,
//! so they share one module; the role modules keep only inherent code, which points
//! here and nowhere back.

use std::borrow::Cow;
use std::future::{self, Future};
use std::sync::Arc;

use sqlx_core::Either;
use sqlx_core::Url;
use sqlx_core::arguments::Arguments;
use sqlx_core::column::{Column, ColumnIndex};
use sqlx_core::connection::Connection;
use sqlx_core::database::Database;
use sqlx_core::encode::Encode;
use sqlx_core::error::{BoxDynError, Error};
use sqlx_core::row::Row;
use sqlx_core::sql_str::SqlStr;
use sqlx_core::statement::Statement;
use sqlx_core::transaction::{Transaction, TransactionManager};
use sqlx_core::types::Type;
use sqlx_core::value::{Value, ValueRef};
use sqlx_durable_object_core::{Value as CoreValue, add_argument};

use crate::error::unsupported;
use crate::{DoArgumentValue, DoQueryResult, DoTypeInfo};

pub(crate) const NO_TRANSACTIONS: &str = "Durable Object storage cannot begin a transaction here; \
     run the statements inside `DoConnection::transaction`, which takes them as a callback";

/// The parameters bound to one query, in placeholder order.
///
/// `format_placeholder` keeps its default, `?`, which is what Durable Object storage expects.
#[derive(Debug, Default, Clone)]
pub struct DoArguments {
    pub(crate) values: Vec<DoArgumentValue>,
}

/// A column of a Durable Object storage result.
#[derive(Debug, Clone)]
pub struct DoColumn {
    pub(crate) name: String,
    pub(crate) ordinal: usize,
    /// The type of the column's first non-NULL value, or `Null` if it has
    /// none -- Durable Object storage reports no declared types (see [`DoTypeInfo`]).
    pub(crate) type_info: DoTypeInfo,
}

/// A connection to a Durable Object's SQL storage.
///
/// There is no socket behind it and nothing to pool: the database lives in
/// the Durable Object itself. Build one from the object's storage with
/// [`DoConnection::new`], in its constructor or per request.
///
/// Both `&mut DoConnection` and `&DoConnection` are sqlx executors, so a
/// connection kept in the object's struct serves every request through
/// `&self`.
pub struct DoConnection {
    /// For [`execute_batch`](Self::execute_batch) and
    /// [`fetch_batch`](Self::fetch_batch), which run through
    /// `Storage::transaction`.
    pub(crate) storage: worker::send::SendWrapper<worker::Storage>,
    /// `storage.sql()`, kept rather than fetched for every query.
    pub(crate) sql: worker::SqlStorage,
}

/// The options type sqlx requires of every connection.
///
/// It parses `do://` so the type is constructible, but
/// [`connect`](ConnectOptions::connect) always fails: a connection comes from
/// a Durable Object's storage, which only the object itself holds. Use
/// [`DoConnection::new`].
///
/// Non-exhaustive, so parsing is the only way to get one -- as sqlx itself
/// does -- and a field can be added without breaking anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DoConnectOptions {
    pub(crate) url: Url,
}

/// A "prepared" Durable Object storage statement: the SQL and nothing else.
///
/// `sql.exec` has no separate prepare that reports parameters or columns, so there
/// is nothing to learn by preparing ahead of time. This exists because sqlx's
/// `Executor::prepare` has to return something.
#[derive(Debug, Clone)]
pub struct DoStatement {
    pub(crate) sql: SqlStr,
}

/// A row of a Durable Object storage result.
pub struct DoRow {
    /// Shared by every row of one result.
    pub(crate) columns: Arc<[DoColumn]>,
    pub(crate) values: Box<[CoreValue]>,
}

/// A value read out of a Durable Object storage result, already converted from
/// JavaScript.
///
/// Owned, because every row is read out of the cursor when the query runs, and
/// there is nothing on the Rust side for a row to borrow from.
///
/// It wraps [`DoArgumentValue`](crate::DoArgumentValue): a value read and
/// a value bound are the same enum, as in sqlx's `Any` driver.
#[derive(Debug, Clone, PartialEq)]
pub struct DoValue(pub(crate) CoreValue);

/// A borrowed [`DoValue`], which is what `Decode` implementations read.
#[derive(Debug, Clone, Copy)]
pub struct DoValueRef<'r>(pub(crate) &'r CoreValue);

/// An open transaction: the executor [`DoConnection::transaction`] hands its
/// callback.
///
/// `&DoTransaction` and `&mut DoTransaction` are sqlx executors, like a
/// connection's references, and every query run through one is part of the
/// transaction.
pub struct DoTransaction {
    pub(crate) sql: worker::SqlStorage,
}

/// Refuses `begin()`.
///
/// `sql.exec` rejects `BEGIN` and `SAVEPOINT` outright: the only transactions
/// Durable Object storage offers wrap a callback, and sqlx's `begin` and
/// `commit` are two separate calls with the caller's code in between.
/// Buffering writes until `commit` would look like a transaction and not be
/// one -- reads inside it would see the database as it was before -- which is
/// worse than an error. So `conn.begin()` compiles and fails at runtime, and
/// transactions go through [`DoConnection::transaction`].
#[derive(Debug)]
pub struct DoTransactionManager;

/// The Cloudflare Durable Object storage driver: the type sqlx code names in
/// `Executor<'_, Database = Do>`, `sqlx::query_as::<Do, _>` and so on.
#[derive(Debug)]
pub struct Do;

impl Database for Do {
    type Connection = DoConnection;

    type TransactionManager = DoTransactionManager;

    type Row = DoRow;

    type QueryResult = DoQueryResult;

    type Column = DoColumn;

    type TypeInfo = DoTypeInfo;

    type Value = DoValue;
    type ValueRef<'r> = DoValueRef<'r>;

    type Arguments = DoArguments;
    type ArgumentBuffer = Vec<DoArgumentValue>;

    type Statement = DoStatement;

    const NAME: &'static str = "Do";

    const URL_SCHEMES: &'static [&'static str] = &["do"];
}

impl Arguments for DoArguments {
    type Database = Do;

    fn reserve(&mut self, additional: usize, _size: usize) {
        self.values.reserve(additional);
    }

    fn add<'t, T>(&mut self, value: T) -> Result<(), BoxDynError>
    where
        T: Encode<'t, Do> + Type<Do>,
    {
        add_argument(&mut self.values, value)
    }

    fn len(&self) -> usize {
        self.values.len()
    }
}

impl Column for DoColumn {
    type Database = Do;

    fn ordinal(&self) -> usize {
        self.ordinal
    }

    fn name(&self) -> &str {
        &self.name
    }

    fn type_info(&self) -> &DoTypeInfo {
        &self.type_info
    }
}

impl Statement for DoStatement {
    type Database = Do;

    fn into_sql(self) -> SqlStr {
        self.sql
    }

    fn sql(&self) -> &SqlStr {
        &self.sql
    }

    fn parameters(&self) -> Option<Either<&[DoTypeInfo], usize>> {
        None
    }

    fn columns(&self) -> &[DoColumn] {
        &[]
    }

    sqlx_core::impl_statement_query!(DoArguments);
}

impl Row for DoRow {
    type Database = Do;

    fn columns(&self) -> &[DoColumn] {
        &self.columns
    }

    fn try_get_raw<I>(&self, index: I) -> Result<DoValueRef<'_>, Error>
    where
        I: ColumnIndex<Self>,
    {
        let index = index.index(self)?;
        self.values
            .get(index)
            .map(DoValueRef)
            .ok_or(Error::ColumnIndexOutOfBounds {
                index,
                len: self.values.len(),
            })
    }
}

impl Connection for DoConnection {
    type Database = Do;

    type Options = DoConnectOptions;

    // Nothing to close: the storage belongs to the Durable Object, not to us.
    fn close(self) -> impl Future<Output = Result<(), Error>> + Send + 'static {
        future::ready(Ok(()))
    }

    fn close_hard(self) -> impl Future<Output = Result<(), Error>> + Send + 'static {
        future::ready(Ok(()))
    }

    // The database is in-process: there is nothing to go stale.
    fn ping(&mut self) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        future::ready(Ok(()))
    }

    /// Always fails -- see [`DoTransactionManager`](crate::DoTransactionManager).
    fn begin(&mut self) -> impl Future<Output = Result<Transaction<'_, Do>, Error>> + Send + '_ {
        Transaction::begin(self, None)
    }

    fn shrink_buffers(&mut self) {}

    fn flush(&mut self) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        future::ready(Ok(()))
    }

    fn should_flush(&self) -> bool {
        false
    }
}

impl TransactionManager for DoTransactionManager {
    type Database = Do;

    fn begin(
        _conn: &mut DoConnection,
        _statement: Option<SqlStr>,
    ) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        future::ready(Err(unsupported(NO_TRANSACTIONS)))
    }

    // Unreachable in practice: `begin` never succeeds, so there is never a
    // transaction to end. They still answer rather than panic.
    fn commit(_conn: &mut DoConnection) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        future::ready(Err(unsupported(NO_TRANSACTIONS)))
    }

    fn rollback(_conn: &mut DoConnection) -> impl Future<Output = Result<(), Error>> + Send + '_ {
        future::ready(Err(unsupported(NO_TRANSACTIONS)))
    }

    // `Transaction` calls this from `Drop` when `begin` failed -- it cannot
    // tell a failed begin from an abandoned transaction -- so it must be a
    // quiet no-op, not an error.
    fn start_rollback(_conn: &mut DoConnection) {}

    fn get_transaction_depth(_conn: &DoConnection) -> usize {
        0
    }
}

impl Value for DoValue {
    type Database = Do;

    fn as_ref(&self) -> DoValueRef<'_> {
        DoValueRef(&self.0)
    }

    fn type_info(&self) -> Cow<'_, DoTypeInfo> {
        Cow::Owned(self.0.type_info())
    }

    fn is_null(&self) -> bool {
        self.0.is_null()
    }
}

impl ValueRef<'_> for DoValueRef<'_> {
    type Database = Do;

    fn to_owned(&self) -> DoValue {
        DoValue(self.0.clone())
    }

    fn type_info(&self) -> Cow<'_, DoTypeInfo> {
        Cow::Owned(self.0.type_info())
    }

    fn is_null(&self) -> bool {
        self.0.is_null()
    }
}
