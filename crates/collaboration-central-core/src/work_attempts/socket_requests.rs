//! A socket request's row: waiting until it is sent, then in flight on one
//! connection until its answer arrives (owed-work design, "A socket request's
//! life"). A waiting row has no `next_attempt_at`, so it is never due: an
//! accepting handler, a Welcome or a freed slot sends it.

use super::{AttemptKind, SocketSend, corrupt, millis, private_id, queries, required};
use durable_steps::{StoreError, Uuid, storage::TransactionScope};
use runtime_primitives::SyncTime;
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// Where a machine's owed requests go: its attached connection and session.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestDestination {
    pub(crate) peer: fleet_protocol::PeerId,
    pub(crate) connection: fleet_protocol::ConnectionId,
    pub(crate) session: fleet_protocol::SessionId,
}

/// What an answer's send id names: the owed row, and where it was sent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SentRequest {
    pub(crate) subject: Uuid,
    pub(crate) kind: AttemptKind,
    pub(crate) connection: Uuid,
    pub(crate) sent_version: Option<u64>,
}

/// Owe a socket request: a waiting row, unless one exists.
pub(crate) async fn owe<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::insert_socket_request_waiting(
        scope,
        Uuid::now_v7().to_string(),
        subject.to_string(),
        kind.as_str().into(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// Put a row back to waiting and forget its send: a late answer is then stale.
pub(crate) async fn rewait<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::reset_socket_request_waiting(
        scope,
        subject.to_string(),
        kind.as_str().into(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// Record a send: in flight on `send.connection` until `retry_at`.
pub(crate) async fn mark_sent<Scope: TransactionScope>(
    scope: &mut Scope,
    subject: Uuid,
    kind: AttemptKind,
    send: &SocketSend,
    retry_at: SyncTime,
    now: SyncTime,
) -> Result<(), StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Encode<'value, Scope::Database>,
{
    queries::mark_socket_request_sent(
        scope,
        subject.to_string(),
        kind.as_str().into(),
        send.send_id.to_string(),
        send.connection.to_string(),
        send.session.to_string(),
        send.sent_version.map(|version| version.to_string()),
        retry_at.unix_millis().to_string(),
        now.unix_millis().to_string(),
    )
    .await
    .map_err(StoreError::backend)
}

/// The row an answer's send id names, if one still does.
pub(crate) async fn sent<Scope: TransactionScope>(
    scope: &mut Scope,
    send_id: Uuid,
) -> Result<Option<SentRequest>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(row) = queries::read_socket_request_by_send(scope, send_id.to_string())
        .await
        .map_err(StoreError::backend)?
    else {
        return Ok(None);
    };
    let sent_version = millis(row.sent_version, row.sent_version_class, "sent_version")?
        .map(|version| u64::try_from(version).map_err(|error| corrupt("sent_version", error)))
        .transpose()?;
    Ok(Some(SentRequest {
        subject: private_id(row.subject, "subject")?,
        kind: AttemptKind::parse(&required(row.kind, "kind")?)?,
        connection: private_id(row.connection_id, "connection_id")?,
        sent_version,
    }))
}

/// How many socket requests are in flight on `connection`.
pub(crate) async fn in_flight_on<Scope: TransactionScope>(
    scope: &mut Scope,
    connection: Uuid,
) -> Result<u32, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let row = queries::count_socket_requests_in_flight(scope, connection.to_string())
        .await
        .map_err(StoreError::backend)?;
    required(row.in_flight, "in_flight")?
        .parse()
        .map_err(|error| corrupt("in_flight", error))
}

/// One page of subjects of `kind` ready to send on `connection`, oldest first
/// and after `after` (subjects are UUIDv7, so this pages in creation order): never sent
/// (or put back to waiting), or in flight on an older connection. A row refused
/// for now waits for its retry time, when the due batch resends it.
pub(crate) async fn owed<Scope: TransactionScope>(
    scope: &mut Scope,
    kind: AttemptKind,
    connection: Uuid,
    after: Option<Uuid>,
    limit: u32,
) -> Result<Vec<Uuid>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    queries::read_owed_socket_requests(
        scope,
        kind.as_str().into(),
        connection.to_string(),
        after.map(|subject| subject.to_string()).unwrap_or_default(),
        limit.to_string(),
    )
    .await
    .map_err(StoreError::backend)?
    .into_iter()
    .map(|row| private_id(row.subject, "subject"))
    .collect()
}
