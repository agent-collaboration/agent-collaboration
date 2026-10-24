//! The control outcome transaction marks the target without rewriting its receipt.
use crate::{StorageError, mail_push_projection::read_mail_push_in_scope};
use collaboration_protocol::{CancelledAfterDelivery, PushId};
use durable_steps::{Host, storage::TransactionScope, turso_store::TursoScope};

pub async fn record_cancelled_after_delivery_in_scope<THost: Host>(
    scope: &mut TursoScope<'_, THost>,
    target: &PushId,
    mark: &CancelledAfterDelivery,
) -> Result<(), StorageError> {
    let Some(record) = read_mail_push_in_scope(scope, target).await? else {
        // Retention is final: a control never recreates its pruned target.
        return Ok(());
    };
    if record.cancelled_after_delivery.is_some() {
        return Ok(());
    }
    mark.validate_for_record(record.kind, record.delivery_state)
        .map_err(|_| StorageError::InvalidRecord)?;
    let encoded = serde_json::to_string(mark).map_err(|_| StorageError::InvalidRecord)?;
    let changed = sqlx_turso::query!(
        "UPDATE router_pushes SET cancelled_after_delivery_json=? WHERE push_id=? AND cancelled_after_delivery_json IS NULL",
        encoded, target.as_str()
    ).execute(scope.executor()).await?;
    if changed.rows_affected() != 1 {
        return Err(StorageError::PushStateConflict);
    }
    Ok(())
}
