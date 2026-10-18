use crate::copy_storage::{decode_record, encode_record, query_statement, statement, text};
use crate::registry_storage::{load_lease, load_transfer, read_record, save_transfer};
use crate::*;
use collaboration_replication_protocol::{
    DirectoryLease, Epoch, LeaseState, ProjectId, RequestId, TransferId,
};
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredUnheld {
    pub unheld: UnheldProject,
    pub change: NonZeroU64,
    pub as_of: RegistryTime,
}

pub(crate) fn unheld(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<StoredUnheld>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_registry_unheld")? {
        return Ok(None);
    }
    let value: Option<StoredUnheld> = read_record(
        tx,
        "SELECT record FROM _registry_unheld WHERE project=?",
        project.as_str(),
    )?;
    if value
        .as_ref()
        .is_some_and(|value| value.unheld.project != *project)
    {
        return Err(crate::fleet_storage::corrupt("unheld identity"));
    }
    Ok(value)
}

pub(crate) fn store_unheld(
    tx: &mut dyn SqlTx,
    unheld: &UnheldProject,
    now: RegistryTime,
) -> Result<(), CentralError> {
    let value = StoredUnheld {
        unheld: unheld.clone(),
        change: NonZeroU64::MIN,
        as_of: now,
    };
    tx.execute(statement(
        "INSERT INTO _registry_unheld(project,record) VALUES(?,?)",
        vec![text(unheld.project.as_str()), text(encode_record(&value)?)],
    ))?;
    Ok(())
}

pub(crate) struct CurrentLeaseContent {
    pub project: ProjectId,
    pub epoch: Epoch,
    pub change: NonZeroU64,
    pub lease: DirectoryLease,
    pub as_of: RegistryTime,
}

pub(crate) fn current(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<CurrentLeaseContent, CentralError> {
    if let Some(lease) = load_lease(tx, project)? {
        return Ok(CurrentLeaseContent {
            project: project.clone(),
            epoch: lease.epoch,
            change: lease.change,
            // Source freshness is the lease mutation, not this retry's clock.
            lease: DirectoryLease::Held {
                holder: lease.holder,
                state: LeaseState::Held,
                expires_at: lease.expires_at,
            },
            as_of: RegistryTime(lease.last_renewed_at),
        });
    }
    let stored = unheld(tx, project)?
        .ok_or_else(|| crate::fleet_storage::corrupt("publication has no current lease state"))?;
    Ok(CurrentLeaseContent {
        project: project.clone(),
        epoch: stored.unheld.import_epoch,
        change: stored.change,
        lease: DirectoryLease::Unheld,
        as_of: stored.as_of,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum PublicationState {
    Dirty {
        retry_at: RegistryTime,
    },
    Offered {
        call_id: ObjectCallId,
        run: RegistryRunId,
        version: LeaseVersion,
        digest: LeaseContentDigest,
        settlement: Option<RequestId>,
        retry_at: RegistryTime,
    },
}
impl PublicationState {
    pub fn retry_at(&self) -> RegistryTime {
        match self {
            Self::Dirty { retry_at } | Self::Offered { retry_at, .. } => *retry_at,
        }
    }
    pub fn call_id(&self) -> Option<&ObjectCallId> {
        match self {
            Self::Offered { call_id, .. } => Some(call_id),
            Self::Dirty { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LeasePublication {
    pub project: ProjectId,
    pub state: PublicationState,
}

pub(crate) fn save(tx: &mut dyn SqlTx, publication: &LeasePublication) -> Result<(), CentralError> {
    let call_id = publication.state.call_id().map_or(
        collaboration_replication_protocol::SqlValue::Null,
        |call_id| text(call_id.as_str()),
    );
    tx.execute(statement("INSERT INTO _registry_lease_publications(project,call_id,record) VALUES(?,?,?) ON CONFLICT(project) DO UPDATE SET call_id=excluded.call_id,record=excluded.record", vec![text(publication.project.as_str()), call_id, text(encode_record(publication)?)]))?;
    Ok(())
}

pub(crate) fn owed(tx: &mut dyn SqlTx) -> Result<Vec<LeasePublication>, CentralError> {
    if !crate::registry_dispatch::has_table(tx, "_registry_lease_publications")? {
        return Ok(Vec::new());
    }
    tx.query(query_statement(
        "SELECT project,call_id,record FROM _registry_lease_publications ORDER BY project",
        Vec::new(),
        ["project", "call_id", "record"],
    ))?
    .iter()
    .map(|row| {
        let publication: LeasePublication =
            decode_record(row.text("record")?, "lease publication")?;
        let call_matches = match (row.value("call_id")?, publication.state.call_id()) {
            (collaboration_replication_protocol::SqlValue::Null, None) => true,
            (collaboration_replication_protocol::SqlValue::Text(stored), Some(call_id)) => {
                stored == call_id.as_str()
            }
            _ => false,
        };
        if publication.project.as_str() != row.text("project")? || !call_matches {
            return Err(crate::fleet_storage::corrupt("lease publication identity"));
        }
        Ok(publication)
    })
    .collect()
}

pub(crate) fn remove(tx: &mut dyn SqlTx, project: &ProjectId) -> Result<(), CentralError> {
    tx.execute(statement(
        "DELETE FROM _registry_lease_publications WHERE project=?",
        vec![text(project.as_str())],
    ))?;
    Ok(())
}

pub(crate) fn settlement(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<RequestId>, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT record FROM _registry_transfers WHERE project=?",
        vec![text(project.as_str())],
        ["record"],
    ))?;
    let mut settlement = None;
    for row in rows {
        let transfer: TransferRecord = decode_record(row.text("record")?, "claim settlement")?;
        if transfer.project != *project {
            return Err(crate::fleet_storage::corrupt("claim settlement project"));
        }
        if let FleetCleanup::Owed(ClaimCleanupDebt::Granted {
            project: stored,
            attempt,
        }) = transfer.fleet_cleanup
        {
            if stored != *project
                || transfer.project != *project
                || transfer.transfer_id.as_str() != attempt.as_str()
                || settlement.is_some()
            {
                return Err(crate::fleet_storage::corrupt("claim settlement identity"));
            }
            settlement = Some(attempt);
        }
    }
    Ok(settlement)
}

pub(crate) fn acknowledge_settlement(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    attempt: &RequestId,
) -> Result<(), CentralError> {
    let id = TransferId::new(attempt.as_str()).map_err(|_| CentralError::ReplyContract)?;
    let mut transfer = load_transfer(tx, &id)?.ok_or(CentralError::ReplyContract)?;
    if transfer.project != *project {
        return Err(CentralError::ReplyContract);
    }
    if matches!(&transfer.fleet_cleanup, FleetCleanup::Owed(ClaimCleanupDebt::Granted { project: stored, attempt: stored_attempt }) if stored == project && stored_attempt == attempt)
    {
        transfer.fleet_cleanup = FleetCleanup::Acknowledged;
        save_transfer(tx, &transfer)?;
    }
    Ok(())
}
