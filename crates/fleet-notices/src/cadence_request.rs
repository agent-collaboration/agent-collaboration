//! Plan the actor and pending-window updates for a cadence request.
use crate::{
    recipient_cadence::notice_due_with_cadence, window_rows::notification_windows,
    window_statements::timestamp,
};
use central_storage::{GuardedStatement, StoreFailure};
use fleet_runtime_ports::{
    CentralStore,
    guarded_statements::{exact, text},
};
use workspace_model::{ActorIdentityId, NoticeCadence, Timestamp};

/// The caller holds the central turn and has admitted the requesting session.
pub async fn plan_notice_cadence_held(
    host: &impl CentralStore,
    actor: &ActorIdentityId,
    cadence: NoticeCadence,
    now: Timestamp,
) -> Result<Vec<GuardedStatement>, StoreFailure> {
    let mut statements = vec![fleet_workspace_access::plan_notice_cadence(
        actor, cadence, now,
    )?];
    // Recompute the pending deadline from now on the new epoch cadence;
    // the actual policy and deadline writes share this request's ledger batch.
    for window in notification_windows(host)
        .await?
        .into_iter()
        .filter(|window| &window.recipient == actor)
    {
        let due = notice_due_with_cadence(now, cadence)?;
        statements.push(exact(
            "UPDATE mail_notification_windows SET due_at=?,updated_at=? WHERE window_id=?",
            vec![text(timestamp(due)), text(timestamp(now)), text(&window.id)],
        ));
    }
    Ok(statements)
}
