//! Snapshot-derived views own no records and cannot admit a write.
mod card_projection;
#[cfg(test)]
mod context_projection_tests;
mod pending_projection;
mod view_snapshot;
mod work_projection;
pub(crate) use card_projection::continuation_card;
pub(crate) use pending_projection::{lead_owed, owner_waiting};
pub(crate) use view_snapshot::{ViewReadSelection, ViewSnapshot};
pub(crate) use work_projection::work_view;

/// Resolve the selector from operands in the caller's existing project transaction.
pub(crate) async fn resolve(
    write: &mut message_board_storage::ProjectWriteTransaction,
    selector: &message_board::Assignment,
) -> Result<message_board::RoleResolution, crate::ProjectAdmissionError> {
    let subject = selector.subject();
    let revision = write.assignment_revision(&subject).await?;
    let actors: std::collections::BTreeSet<_> = write
        .active_roles(&subject, selector.role())
        .await?
        .into_iter()
        .map(|assignment| assignment.holder)
        .collect();
    Ok(if actors.is_empty() {
        message_board::RoleResolution::NoHolder { revision }
    } else {
        message_board::RoleResolution::Holders { actors, revision }
    })
}
