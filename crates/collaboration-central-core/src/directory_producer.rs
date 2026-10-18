use crate::copy_storage::{decode_record, encode_record, query_statement, statement, text};
use crate::*;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PendingDirectoryUpdate {
    call_id: ObjectCallId,
    update: DirectoryUpdate,
    retry_at: RegistryTime,
}
fn save(tx: &mut dyn SqlTx, pending: &PendingDirectoryUpdate) -> Result<(), CentralError> {
    tx.execute(statement("INSERT INTO _copy_directory_outbox(update_key,call_id,record) VALUES(?,?,?) ON CONFLICT(update_key) DO UPDATE SET call_id=excluded.call_id,record=excluded.record",vec![text(pending.update.key()),text(pending.call_id.as_str()),text(encode_record(pending)?)]))?;
    Ok(())
}
fn dispatch<Outgoing, Reply>(pending: PendingDirectoryUpdate) -> KindEffect<Outgoing, Reply>
where
    Outgoing: From<Dispatch<FleetKind>>,
{
    KindEffect::Dispatch(
        Dispatch::<FleetKind> {
            call_id: pending.call_id,
            to: (),
            call: FleetCall::DirectoryUpdate(pending.update),
            carry: None,
        }
        .into(),
    )
}
pub(crate) fn publish<Outgoing, Reply>(
    tx: &mut dyn SqlTx,
    update: DirectoryUpdate,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<KindEffect<Outgoing, Reply>>, CentralError>
where
    Outgoing: From<Dispatch<FleetKind>>,
{
    let pending = PendingDirectoryUpdate {
        call_id: ids
            .call_ids
            .pop()
            .ok_or_else(|| CentralError::InvalidInput {
                message: "missing directory update ID".into(),
            })?,
        update,
        retry_at: RegistryTime(now.0 + chrono::TimeDelta::seconds(5)),
    };
    save(tx, &pending)?;
    let mut effects = Vec::new();
    if let Some(at) = next_alarm(tx)? {
        effects.push(KindEffect::SetAlarm { at });
    }
    effects.push(dispatch(pending));
    Ok(effects)
}
pub(crate) fn acknowledged(tx: &mut dyn SqlTx, id: &ObjectCallId) -> Result<(), CentralError> {
    if crate::registry_dispatch::has_table(tx, "_copy_directory_outbox")? {
        tx.execute(statement(
            "DELETE FROM _copy_directory_outbox WHERE call_id=?",
            vec![text(id.as_str())],
        ))?;
    }
    Ok(())
}
fn pending(tx: &mut dyn SqlTx) -> Result<Vec<PendingDirectoryUpdate>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_copy_directory_outbox")? {
        return Ok(Vec::new());
    }
    tx.query(query_statement(
        "SELECT record FROM _copy_directory_outbox",
        Vec::new(),
        ["record"],
    ))?
    .iter()
    .map(|row| decode_record(row.text("record")?, "directory update"))
    .collect()
}
pub(crate) fn alarm<Outgoing, Reply>(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<Vec<KindEffect<Outgoing, Reply>>, CentralError>
where
    Outgoing: From<Dispatch<FleetKind>>,
{
    let mut effects = Vec::new();
    for mut pending in pending(tx)? {
        if pending.retry_at.0 <= now.0 {
            pending.retry_at = RegistryTime(now.0 + chrono::TimeDelta::seconds(5));
            save(tx, &pending)?;
            effects.push(dispatch(pending));
        }
    }
    if let Some(at) = next_alarm(tx)? {
        effects.insert(0, KindEffect::SetAlarm { at });
    }
    Ok(effects)
}
pub(crate) fn next_alarm(tx: &mut dyn SqlTx) -> Result<Option<RegistryTime>, CentralError> {
    Ok(pending(tx)?
        .into_iter()
        .map(|pending| pending.retry_at)
        .min_by_key(|at| at.0))
}

pub(crate) fn summary(
    tx: &mut dyn SqlTx,
    project: &collaboration_replication_protocol::ProjectId,
    summary: &collaboration_replication_protocol::ProjectSummary,
    position: collaboration_replication_protocol::ReplicationPosition,
    now: RegistryTime,
    ids: &mut IdBatch,
) -> Result<Vec<CopyEffect>, CentralError> {
    use crate::copy_storage::unsigned;
    tx.execute(statement("INSERT INTO _copy_summary(singleton,status,latest_record_at,position_epoch,position_commit,acked_at) VALUES(1,?,?,?,?,?) ON CONFLICT(singleton) DO UPDATE SET status=excluded.status,latest_record_at=excluded.latest_record_at,position_epoch=excluded.position_epoch,position_commit=excluded.position_commit,acked_at=excluded.acked_at",vec![text(summary.status.clone()),text(summary.latest_record_at.to_rfc3339()),unsigned(position.epoch.value()),unsigned(position.commit_id.value()),text(now.0.to_rfc3339())]))?;
    publish(
        tx,
        DirectoryUpdate::Summary {
            project: project.clone(),
            summary: summary.clone(),
            position,
            acked_at: now,
        },
        now,
        ids,
    )
}
