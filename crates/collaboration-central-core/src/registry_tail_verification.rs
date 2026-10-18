//! Registry history is the authority for who wrote a reported epoch.
use crate::copy_storage::{query_statement, text, unsigned};
use crate::registry_storage::load_lease;
use crate::{CentralError, SqlTx, TailVerificationRefusal, TailVerificationResult};
use collaboration_replication_protocol::{Epoch, InstallationId, ProjectId};

pub(crate) fn verify(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
    epoch: Epoch,
    installation: &InstallationId,
) -> Result<TailVerificationResult, CentralError> {
    let current = load_lease(tx, project)?;
    let refused = || {
        TailVerificationResult::Refused(TailVerificationRefusal::NotWriter {
            current: current.as_ref().map(|lease| lease.epoch),
        })
    };
    let rows = tx.query(query_statement(
        "SELECT installation FROM _registry_history WHERE project=? AND epoch=?",
        vec![text(project.as_str()), unsigned(epoch.value())],
        ["installation"],
    ))?;
    let Some(row) = rows.first() else {
        return Ok(refused());
    };
    let stored = InstallationId::new(row.text("installation")?)
        .map_err(|_| crate::fleet_storage::corrupt("tail history installation"))?;
    if stored.as_str() != row.text("installation")? {
        return Err(crate::fleet_storage::corrupt("tail history installation"));
    }
    let lease = current
        .as_ref()
        .ok_or_else(|| crate::fleet_storage::corrupt("tail current holder"))?;
    if epoch > lease.epoch {
        return Err(crate::fleet_storage::corrupt("tail history epoch"));
    }
    if stored != *installation {
        return Ok(refused());
    }
    Ok(TailVerificationResult::Verified {
        current_holder: lease.holder.clone(),
    })
}
