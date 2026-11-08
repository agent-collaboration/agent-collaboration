//! How far each machine's Router has applied its notice stream (NoticesApplied).
//!
//! `applied_through` is domain state on the machine's progress row: it only
//! ever increases, and a Router can't acknowledge past what was issued. Whether
//! central still owes that Router a resend is attempt state, kept elsewhere.

use crate::{
    fleet_record_codec::{corrupt, required},
    generated_machine_notices as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use fleet_protocol::{NoticePosition, PeerId, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// One machine's notice stream as its Router last acknowledged it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NoticeProgress {
    /// The last position central issued to this machine.
    pub(crate) issued: u64,
    /// The last position the Router acknowledged; `None` before any acknowledgment.
    pub(crate) applied: Option<u64>,
}

impl NoticeProgress {
    /// Whether the Router has notices it hasn't acknowledged.
    pub(crate) fn behind(self) -> bool {
        self.applied.unwrap_or(0) < self.issued
    }
}

/// The machine's progress, or `None` if central never issued it a notice.
pub(crate) async fn progress<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
) -> Result<Option<NoticeProgress>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let mut rows = queries::read_machine_notice_applied(scope, peer.to_string())
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt("machine_notices.progress", "multiple stream heads"));
    }
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    let issued = position(
        Some(required(
            row.last_issued_position,
            "machine_notices.issued",
        )?),
        row.issued_class,
        "machine_notices.issued",
    )?
    .ok_or_else(|| corrupt("machine_notices.issued", "required column is NULL"))?;
    let applied = position(
        row.applied_through,
        row.applied_class,
        "machine_notices.applied_through",
    )?;
    if applied.is_some_and(|applied| applied > issued) {
        return Err(corrupt(
            "machine_notices.applied_through",
            "acknowledged past the issued head",
        ));
    }
    Ok(Some(NoticeProgress { issued, applied }))
}

/// Record that the Router applied through `through`. A lower value than the
/// stored one is ignored; a value past the issued head is clamped to it.
pub(crate) async fn record_applied<Scope: TransactionScope>(
    scope: &mut Scope,
    peer: PeerId,
    through: NoticePosition,
    now: SyncTime,
) -> Result<Option<NoticeProgress>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String: Type<Scope::Database> + Encode<'value, Scope::Database>,
    for<'value> Option<String>: Type<Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let Some(before) = progress(scope, peer).await? else {
        return Ok(None);
    };
    let through = through.value().min(before.issued);
    queries::record_machine_notice_applied(
        scope,
        through.to_string(),
        now.unix_millis().to_string(),
        peer.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    progress(scope, peer).await
}

/// A canonical non-negative SQLite integer, or `None` for SQL NULL.
fn position(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<u64>, StoreError> {
    match (value, class.as_deref()) {
        (None, Some("null")) => Ok(None),
        (Some(value), Some("integer")) => {
            let parsed = value
                .parse::<u64>()
                .map_err(|error| corrupt(field, error))?;
            if parsed.to_string() != value || i64::try_from(parsed).is_err() {
                return Err(corrupt(field, "position is not a canonical SQLite integer"));
            }
            Ok(Some(parsed))
        }
        _ => Err(corrupt(field, "column is not an integer or NULL")),
    }
}

#[cfg(test)]
#[path = "machine_notice_progress_test.rs"]
mod tests;
