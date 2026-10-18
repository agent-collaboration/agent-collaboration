use crate::{CentralError, SqlTx};
use fleet_admission::{DirectoryEntry, DirectoryQuery, DirectorySide};
use message_board::Record;

pub(crate) fn read(
    tx: &mut dyn SqlTx,
    query: &DirectoryQuery,
) -> Result<Vec<DirectoryEntry>, CentralError> {
    let mut entries = Vec::new();
    for record in crate::fleet_storage::all_records(tx)? {
        let Record::DirectoryEntry(identity) = record else {
            continue;
        };
        let Some(Record::Board(board)) =
            crate::fleet_storage::read_record(tx, &identity.board.into())?
        else {
            return Err(crate::fleet_storage::corrupt("directory board"));
        };
        if board.workspace_id != identity.envelope.scope.workspace {
            return Err(crate::fleet_storage::corrupt("directory workspace"));
        }
        let project = collaboration_replication_protocol::ProjectId::new(identity.id.as_str())
            .map_err(|_| crate::fleet_storage::corrupt("directory project"))?;
        let entry = DirectoryEntry {
            project: identity.id,
            name: identity.title,
            workspace: board.workspace_id,
            board: board.board_id,
            board_archive: board.state,
            copy_side: crate::directory_receiver::copy_metadata(tx, &project)?,
            lease_side: crate::directory_receiver::lease_metadata(tx, &project)?,
        };
        if matches_query(&entry, query) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn matches_query(entry: &DirectoryEntry, query: &DirectoryQuery) -> bool {
    if query.text.as_ref().is_some_and(|text| {
        !entry
            .name
            .as_str()
            .to_lowercase()
            .contains(&text.to_lowercase())
    }) || query
        .workspace
        .as_ref()
        .is_some_and(|workspace| workspace != &entry.workspace)
        || query
            .board
            .as_ref()
            .is_some_and(|board| board != &entry.board)
    {
        return false;
    }
    if let Some(status) = &query.status {
        match &entry.copy_side {
            DirectorySide::Known(metadata) if &metadata.status == status => {}
            DirectorySide::Known(_) | DirectorySide::NotYetReported => return false,
        }
    }
    if let Some(holder) = &query.holder {
        match &entry.lease_side {
            DirectorySide::Known(fleet_admission::LeaseMetadata {
                lease:
                    collaboration_replication_protocol::DirectoryLease::Held { holder: actual, .. },
                ..
            }) if actual == holder => {}
            DirectorySide::Known(_) | DirectorySide::NotYetReported => return false,
        }
    }
    true
}
