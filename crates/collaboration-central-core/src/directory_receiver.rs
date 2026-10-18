use crate::copy_storage::{encode_record, statement, text};
use crate::{CentralError, DirectoryUpdate, SqlTx};
use collaboration_replication_protocol::ProjectId;
use fleet_admission::{CopyMetadata, CopyMetadataAsOf, DirectorySide, LeaseMetadata};

fn read_side(
    tx: &mut dyn SqlTx,
    table: &str,
    project: &ProjectId,
) -> Result<Option<DirectoryUpdate>, CentralError> {
    // Callers select one of two fixed, private table names; project is bound.
    crate::registry_storage::read_record(
        tx,
        &format!("SELECT record FROM {table} WHERE project=?"),
        project.as_str(),
    )
}

pub(crate) fn copy_metadata(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<DirectorySide<CopyMetadata>, CentralError> {
    match read_side(tx, "directory_status", project)? {
        None => Ok(DirectorySide::NotYetReported),
        Some(DirectoryUpdate::Summary {
            project: stored,
            summary,
            position,
            acked_at,
        }) if stored == *project => Ok(DirectorySide::Known(CopyMetadata {
            status: summary.status,
            latest_record_at: summary.latest_record_at,
            as_of: CopyMetadataAsOf {
                position,
                acked_at: acked_at.0,
            },
        })),
        Some(_) => Err(crate::fleet_storage::corrupt("directory summary")),
    }
}

pub(crate) fn lease_metadata(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<DirectorySide<LeaseMetadata>, CentralError> {
    match lease_update(tx, project)? {
        None => Ok(DirectorySide::NotYetReported),
        Some(DirectoryUpdate::Lease {
            project: stored,
            lease,
            as_of,
            ..
        }) if stored == *project => Ok(DirectorySide::Known(LeaseMetadata {
            lease,
            as_of: as_of.0,
        })),
        Some(_) => Err(crate::fleet_storage::corrupt("directory lease")),
    }
}

pub(crate) fn lease_update(
    tx: &mut dyn SqlTx,
    project: &ProjectId,
) -> Result<Option<DirectoryUpdate>, CentralError> {
    let stored = read_side(tx, "directory_lease", project)?;
    if let Some(update) = &stored {
        match update {
            DirectoryUpdate::Lease {
                project: stored_project,
                lease,
                digest,
                as_of,
                ..
            } if stored_project == project
                && crate::LeaseContentDigest::of(project, lease, *as_of)? == *digest => {}
            _ => return Err(crate::fleet_storage::corrupt("directory lease")),
        }
    }
    Ok(stored)
}

pub(crate) fn receive(tx: &mut dyn SqlTx, incoming: &DirectoryUpdate) -> Result<(), CentralError> {
    let (table, project, newer) = match incoming {
        DirectoryUpdate::Summary {
            project, position, ..
        } => {
            let previous = read_side(tx, "directory_status", project)?;
            let newer = match previous {
                None => true,
                Some(DirectoryUpdate::Summary {
                    project: previous_project,
                    position: previous,
                    ..
                }) if previous_project == *project => *position > previous,
                _ => return Err(crate::fleet_storage::corrupt("directory summary")),
            };
            ("directory_status", project, newer)
        }
        DirectoryUpdate::Lease { .. } | DirectoryUpdate::ClaimGranted { .. } => {
            return Err(CentralError::ReplyContract);
        }
    };
    if newer {
        tx.execute(statement(&format!("INSERT INTO {table}(project,record) VALUES(?,?) ON CONFLICT(project) DO UPDATE SET record=excluded.record"),vec![text(project.as_str()),text(encode_record(incoming)?)]))?;
    }
    Ok(())
}
