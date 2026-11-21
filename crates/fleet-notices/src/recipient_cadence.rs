//! The epoch-grid deadline for a recipient's stored or requested cadence.
use central_storage::StoreFailure;
use fleet_runtime_ports::{CentralStore, workspace_decode_error::corrupt};
use fleet_workspace_access::read_notice_cadence;
use workspace_model::{ActorIdentityId, NoticeCadence, Timestamp};

pub(crate) async fn notice_due(
    host: &impl CentralStore,
    recipient: &ActorIdentityId,
    decision_at: Timestamp,
) -> Result<Timestamp, StoreFailure> {
    notice_due_with_cadence(decision_at, read_notice_cadence(host, recipient).await?)
}

pub(crate) fn notice_due_with_cadence(
    decision_at: Timestamp,
    cadence: NoticeCadence,
) -> Result<Timestamp, StoreFailure> {
    central_engine_model::aligned_strictly_after(decision_at, cadence.seconds())
        .ok_or_else(|| corrupt("mail_notification_windows.due_at"))
}
