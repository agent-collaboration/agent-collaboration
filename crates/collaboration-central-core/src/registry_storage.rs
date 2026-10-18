use crate::copy_storage::query_statement;
use crate::copy_storage::{decode_record, encode_record, epoch_field, statement, text, unsigned};
use crate::registry_contract::PendingContinuation;
use crate::*;
use crate::{
    CentralError, ConnectionId, IdBatch, ObjectCallId, RegistryLease, SqlTx, StoreError,
    TransferRecord, TransferState,
};
use chrono::{DateTime, Utc};
use collaboration_replication_protocol::{
    Epoch, InstallationId, ProjectId, ReplicationPosition, RequestId, SchemaSupport, SessionAnswer,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MachineState {
    Enrolled,
    Revoked,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct MachineRecord {
    pub label: String,
    pub installation: InstallationId,
    pub access_client_id: crate::AccessClientId,
    pub state: MachineState,
    pub expires_at: DateTime<Utc>,
    pub schema_support: Option<SchemaSupport>,
    pub last_seen: DateTime<Utc>,
    pub connection: Option<ConnectionId>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RenewalAttempt {
    pub installation: InstallationId,
    pub project: ProjectId,
    pub epoch: Epoch,
    pub answer: SessionAnswer,
    pub decided_at: DateTime<Utc>,
}
pub(crate) fn read_record<T: DeserializeOwned>(
    tx: &mut dyn SqlTx,
    sql: &str,
    key: &str,
) -> Result<Option<T>, CentralError> {
    let rows = tx.query(query_statement(sql, vec![text(key)], ["record"]))?;
    if rows.len() > 1 {
        return Err(StoreError::Corrupt {
            field: "record cardinality".into(),
        }
        .into());
    }
    rows.first()
        .map(|r| decode_record(r.text("record")?, "record"))
        .transpose()
}
pub(crate) fn load_lease(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<RegistryLease>, CentralError> {
    let lease: Option<RegistryLease> = read_record(
        tx,
        "SELECT record FROM _registry_leases WHERE project=?",
        project.as_str(),
    )?;
    if lease
        .as_ref()
        .is_some_and(|l| &l.project != project || l.expires_at < l.last_renewed_at)
    {
        return Err(StoreError::Corrupt {
            field: "lease".into(),
        }
        .into());
    }
    Ok(lease)
}
pub(crate) fn save_lease(tx: &mut dyn SqlTx, lease: &RegistryLease) -> Result<(), CentralError> {
    let mut updated = lease.clone();
    updated.change = match load_lease(tx, &lease.project)? {
        Some(previous) if previous.epoch == lease.epoch => previous
            .change
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt {
                field: "lease change exhausted".into(),
            })?,
        Some(previous) if previous.epoch > lease.epoch => {
            return Err(StoreError::Corrupt {
                field: "lease epoch regression".into(),
            }
            .into());
        }
        _ => std::num::NonZeroU64::MIN,
    };
    tx.execute(statement("INSERT INTO _registry_leases(project,record) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET record=excluded.record",vec![text(lease.project.as_str()),text(encode_record(&updated)?)]))?;
    tx.execute(statement("INSERT INTO _registry_history(project,epoch,installation) VALUES(?,?,?) ON CONFLICT(project,epoch) DO NOTHING",vec![text(lease.project.as_str()),unsigned(lease.epoch.value()),text(lease.holder.as_str())]))?;
    save_floor(tx, &lease.project, lease.epoch)
}
pub(crate) fn save_floor(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    epoch: Epoch,
) -> Result<(), CentralError> {
    if let Some(existing) = load_floor(tx, project)?
        && existing > epoch
    {
        return Err(StoreError::Corrupt {
            field: "epoch floor regression".into(),
        }
        .into());
    }
    tx.execute(statement("INSERT INTO _registry_epoch_floor(project,epoch) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET epoch=excluded.epoch",vec![text(project.as_str()),unsigned(epoch.value())]))?;
    Ok(())
}
pub(crate) fn load_floor(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<Epoch>, CentralError> {
    tx.query(query_statement(
        "SELECT epoch FROM _registry_epoch_floor WHERE project=?",
        vec![text(project.as_str())],
        ["epoch"],
    ))?
    .first()
    .map(|r| epoch_field(r, "epoch").map_err(Into::into))
    .transpose()
}
pub(crate) fn load_machine(
    tx: &mut dyn SqlTx,
    installation: &InstallationId,
) -> Result<Option<MachineRecord>, CentralError> {
    let m: Option<MachineRecord> = read_record(
        tx,
        "SELECT record FROM _registry_machines WHERE installation=?",
        installation.as_str(),
    )?;
    if m.as_ref().is_some_and(|m| &m.installation != installation) {
        return Err(StoreError::Corrupt {
            field: "machine installation".into(),
        }
        .into());
    }
    Ok(m)
}
pub(crate) fn save_machine(tx: &mut dyn SqlTx, m: &MachineRecord) -> Result<(), CentralError> {
    tx.execute(statement(
        "UPDATE _registry_machines SET record=? WHERE installation=?",
        vec![text(encode_record(m)?), text(m.installation.as_str())],
    ))?;
    Ok(())
}
pub(crate) fn load_transfer(
    tx: &mut dyn SqlTx,
    id: &collaboration_replication_protocol::TransferId,
) -> Result<Option<TransferRecord>, CentralError> {
    read_record(
        tx,
        "SELECT record FROM _registry_transfers WHERE transfer_id=?",
        id.as_str(),
    )
}
pub(crate) fn active_transfer(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<TransferRecord>, CentralError> {
    read_record(
        tx,
        "SELECT record FROM _registry_transfers WHERE project=? AND active=1",
        project.as_str(),
    )
}
pub(crate) fn save_transfer(tx: &mut dyn SqlTx, t: &TransferRecord) -> Result<(), CentralError> {
    let active = matches!(
        t.state,
        TransferState::Draining | TransferState::Fencing { .. } | TransferState::Reserving { .. }
    );
    tx.execute(statement("INSERT INTO _registry_transfers(transfer_id,project,active,record) VALUES(?,?,?,?) ON CONFLICT(transfer_id) DO UPDATE SET active=excluded.active,record=excluded.record",vec![text(t.transfer_id.as_str()),text(t.project.as_str()),collaboration_replication_protocol::SqlValue::Integer(i64::from(active)),text(encode_record(t)?)]))?;
    Ok(())
}
pub(crate) fn cutoff(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    epoch: Epoch,
    installation: &InstallationId,
) -> Result<Option<ReplicationPosition>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT installation,cutoff FROM _registry_history WHERE project=? AND epoch=?",
        vec![text(project.as_str()), unsigned(epoch.value())],
        ["installation", "cutoff"],
    ))?;
    let Some(row) = rows.first() else {
        return Ok(None);
    };
    if row.text("installation")? != installation.as_str() {
        return Ok(None);
    }
    match row.value("cutoff")? {
        collaboration_replication_protocol::SqlValue::Null => Ok(None),
        collaboration_replication_protocol::SqlValue::Text(v) => {
            decode_record(v, "cutoff").map(Some)
        }
        _ => Err(StoreError::Corrupt {
            field: "cutoff".into(),
        }
        .into()),
    }
}
pub(crate) fn continuation_id(
    tx: &mut dyn SqlTx,
    ids: &mut IdBatch,
    pending: PendingContinuation,
) -> Result<ObjectCallId, CentralError> {
    let id = ids
        .call_ids
        .pop()
        .ok_or_else(|| CentralError::InvalidInput {
            message: "shell supplied no call ID".into(),
        })?;
    tx.execute(statement(
        "INSERT INTO _central_continuations(call_id,record) VALUES(?,?)",
        vec![text(id.as_str()), text(encode_record(&pending)?)],
    ))?;
    Ok(id)
}
pub(crate) fn dispatch_copy(
    tx: &mut dyn SqlTx,
    ids: &mut IdBatch,
    to: ProjectId,
    call: CopyCall,
    pending: PendingContinuation,
) -> Result<RegistryEffect, CentralError> {
    Ok(RegistryEffect::Dispatch(RegistryDispatch::Copy(Dispatch {
        call_id: continuation_id(tx, ids, pending)?,
        to,
        call,
        carry: None,
    })))
}
pub(crate) fn session_dispatch<To: ObjectKind>(
    tx: &mut dyn SqlTx,
    ids: &mut IdBatch,
    to: To::Address,
    call: To::Call,
    pending: PendingContinuation,
) -> Result<SessionEffect, CentralError>
where
    SessionKind: MayCall<To>,
    SessionDispatch: From<Dispatch<To>>,
{
    Ok(SessionEffect::Dispatch(
        Dispatch::<To> {
            call_id: continuation_id(tx, ids, pending)?,
            to,
            call,
            carry: None,
        }
        .into(),
    ))
}
pub(crate) fn take_continuation(
    tx: &mut dyn SqlTx,
    id: &ObjectCallId,
) -> Result<Option<PendingContinuation>, CentralError> {
    let pending = read_record(
        tx,
        "SELECT record FROM _central_continuations WHERE call_id=?",
        id.as_str(),
    )?;
    if pending.is_some() {
        tx.execute(statement(
            "DELETE FROM _central_continuations WHERE call_id=?",
            vec![text(id.as_str())],
        ))?;
    }
    Ok(pending)
}
pub(crate) fn save_attempt(
    tx: &mut dyn SqlTx,
    id: &RequestId,
    attempt: &RenewalAttempt,
) -> Result<(), CentralError> {
    tx.execute(statement(
        "INSERT INTO _registry_renewal_attempts(attempt,record) VALUES(?,?)",
        vec![text(id.as_str()), text(encode_record(attempt)?)],
    ))?;
    Ok(())
}
