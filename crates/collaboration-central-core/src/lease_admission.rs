//! Fleet's outside witness orders registry publications, never lease decisions.
use crate::copy_storage::{encode_record, query_statement, statement, text, unsigned};
use crate::{
    CentralError, DirectoryUpdate, LeaseContentDigest, LeaseUpdateResult, RegistryIncarnation,
    RegistryTime, SqlTx,
};

pub(crate) fn fence(tx: &mut dyn SqlTx) -> Result<u64, CentralError> {
    let rows = tx.query(query_statement(
        "SELECT incarnation FROM lease_incarnation_fence WHERE singleton=1",
        Vec::new(),
        ["incarnation"],
    ))?;
    rows.first()
        .ok_or_else(|| crate::fleet_storage::corrupt("lease incarnation fence"))?
        .unsigned("incarnation")
        .map_err(Into::into)
}

pub(crate) fn advance(
    tx: &mut dyn SqlTx,
    at_least: u64,
) -> Result<RegistryIncarnation, CentralError> {
    let next =
        fence(tx)?
            .max(at_least)
            .checked_add(1)
            .ok_or_else(|| CentralError::InvalidInput {
                message: "lease incarnation exhausted".into(),
            })?;
    let incarnation = RegistryIncarnation::new(next)
        .map_err(|_| crate::fleet_storage::corrupt("lease incarnation"))?;
    tx.execute(statement(
        "UPDATE lease_incarnation_fence SET incarnation=? WHERE singleton=1",
        vec![unsigned(next)],
    ))?;
    Ok(incarnation)
}

pub(crate) fn receive(
    tx: &mut dyn SqlTx,
    update: &DirectoryUpdate,
    now: RegistryTime,
) -> Result<LeaseUpdateResult, CentralError> {
    let (project, lease, version, digest, as_of, settlement) = match update {
        DirectoryUpdate::Lease {
            project,
            lease,
            lease_version,
            digest,
            as_of,
        } => (project, lease, *lease_version, digest, *as_of, None),
        DirectoryUpdate::ClaimGranted {
            project,
            attempt,
            lease,
            lease_version,
            digest,
            as_of,
        } => (
            project,
            lease,
            *lease_version,
            digest,
            *as_of,
            Some(attempt),
        ),
        DirectoryUpdate::Summary { .. } => return Err(CentralError::ReplyContract),
    };
    let current_fence = fence(tx)?;
    if version.incarnation().value() < current_fence {
        return Ok(LeaseUpdateResult::Fenced {
            fence: RegistryIncarnation::new(current_fence)
                .map_err(|_| crate::fleet_storage::corrupt("lease incarnation fence"))?,
        });
    }
    if LeaseContentDigest::of(project, lease, as_of)? != *digest {
        return Err(CentralError::InvalidInput {
            message: "lease content digest mismatch".into(),
        });
    }
    let previous = crate::directory_receiver::lease_update(tx, project)?;
    let result = match previous {
        Some(DirectoryUpdate::Lease {
            lease_version: stored,
            ..
        }) if version < stored => LeaseUpdateResult::Older { stored },
        Some(DirectoryUpdate::Lease {
            lease_version: stored,
            digest: stored_digest,
            ..
        }) if version == stored => {
            if *digest == stored_digest {
                LeaseUpdateResult::Duplicate
            } else {
                LeaseUpdateResult::Conflict { stored }
            }
        }
        Some(DirectoryUpdate::Summary { .. } | DirectoryUpdate::ClaimGranted { .. }) => {
            return Err(crate::fleet_storage::corrupt("lease projection shape"));
        }
        _ => LeaseUpdateResult::Applied,
    };
    if matches!(result, LeaseUpdateResult::Conflict { .. }) {
        return Ok(result);
    }
    if result == LeaseUpdateResult::Applied {
        let normalized = DirectoryUpdate::Lease {
            project: project.clone(),
            lease: lease.clone(),
            lease_version: version,
            digest: digest.clone(),
            as_of,
        };
        tx.execute(statement("INSERT INTO directory_lease(project,record) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET record=excluded.record", vec![text(project.as_str()), text(encode_record(&normalized)?)]))?;
    }
    if let Some(attempt) = settlement {
        crate::claim_reservation::grant(tx, project, attempt)?;
    }
    if result == LeaseUpdateResult::Applied || settlement.is_some() {
        crate::archive_receiver::complete_project(tx, project, now)?;
    }
    Ok(result)
}
