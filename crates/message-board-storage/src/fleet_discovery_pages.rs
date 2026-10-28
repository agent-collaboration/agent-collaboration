//! Discovery filters metadata in key order before the application requests hit bodies.
use super::{FleetCacheSnapshot, record_decode_error};
use crate::{
    RouterLocalStorageError, fleet_record_codec::decode_fleet_reference,
    project_record_io::record_kind_tag,
};
use message_board::{
    BoardId, DiscoveryKind, DiscoveryScope, DiscoverySearchRequest, RecordKind, RecordRef,
    WorkspaceId,
};

pub struct FleetDiscoveryKey {
    pub reference: RecordRef,
    pub workspace: WorkspaceId,
    pub board: Option<BoardId>,
}
impl FleetDiscoveryKey {
    pub fn sort_key(&self) -> String {
        let prefix = match self.reference.kind {
            RecordKind::Workspace => "0:",
            RecordKind::Board => "1:",
            _ => "2:",
        };
        format!("{prefix}{}", self.reference.id.as_str())
    }
}
impl FleetCacheSnapshot {
    pub async fn discovery_page_keys(
        &mut self,
        request: &DiscoverySearchRequest,
        after: &str,
        limit: u64,
    ) -> Result<Vec<FleetDiscoveryKey>, RouterLocalStorageError> {
        let limit = i64::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?;
        let query = request.query.as_str().to_ascii_lowercase();
        let archived = request.include_archived;
        let mut keys = Vec::new();
        for (kind, prefix) in [
            (RecordKind::Workspace, "0:"),
            (RecordKind::Board, "1:"),
            (RecordKind::Project, "2:"),
        ] {
            let included = matches!(
                (request.kind, kind),
                (DiscoveryKind::All, _)
                    | (DiscoveryKind::Workspace, RecordKind::Workspace)
                    | (DiscoveryKind::Board, RecordKind::Board)
                    | (DiscoveryKind::ProjectDirectoryEntry, RecordKind::Project)
            );
            if !included || (!after.is_empty() && after > prefix && !after.starts_with(prefix)) {
                continue;
            }
            let after = after.strip_prefix(prefix).unwrap_or_default();
            let tag = record_kind_tag(kind);
            // Map each checked query's generated row at the boundary; the columns are identical.
            macro_rules! collect_keys {
                ($rows:expr) => {
                    for row in $rows {
                        keys.push(FleetDiscoveryKey {
                            reference: decode_fleet_reference(&row.kind, row.id)
                                .map_err(record_decode_error)?,
                            workspace: WorkspaceId::try_from(row.workspace)
                                .map_err(|_| RouterLocalStorageError::InvalidRecord)?,
                            board: row
                                .board
                                .map(BoardId::try_from)
                                .transpose()
                                .map_err(|_| RouterLocalStorageError::InvalidRecord)?,
                        });
                    }
                };
            }
            match &request.scope {
                DiscoveryScope::AllWorkspaces => collect_keys!(
                    sqlx_turso::query_file!(
                        "src/fleet_discovery_all.sql",
                        tag,
                        after,
                        query,
                        archived,
                        limit
                    )
                    .fetch_all(&mut *self.transaction)
                    .await?
                ),
                DiscoveryScope::Workspace { workspace_id } => {
                    let workspace = workspace_id.as_str();
                    collect_keys!(
                        sqlx_turso::query_file!(
                            "src/fleet_discovery_workspace.sql",
                            tag,
                            after,
                            workspace,
                            query,
                            archived,
                            limit
                        )
                        .fetch_all(&mut *self.transaction)
                        .await?
                    );
                }
                DiscoveryScope::Board { board_id } => {
                    let board = board_id.as_str();
                    collect_keys!(
                        sqlx_turso::query_file!(
                            "src/fleet_discovery_board.sql",
                            tag,
                            after,
                            board,
                            query,
                            archived,
                            limit
                        )
                        .fetch_all(&mut *self.transaction)
                        .await?
                    );
                }
            }
        }
        keys.sort_by_key(FleetDiscoveryKey::sort_key);
        keys.truncate(usize::try_from(limit).map_err(|_| RouterLocalStorageError::InvalidRecord)?);
        Ok(keys)
    }
}
