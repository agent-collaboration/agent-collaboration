//! Fleet discovery reads only the metadata projection carried by FleetClient.
use super::board_paging::fingerprint;
use super::{BoardOperations, admission_error};
use message_board::{
    BoardError, DiscoveryHit, DiscoverySearchRequest, DiscoverySearchResult, Record, RecordRef,
    RepositoryListRequest, RepositoryListResult, WorkspaceShowRequest,
};
impl BoardOperations {
    pub async fn repository_list(
        &self,
        request: RepositoryListRequest,
    ) -> Result<RepositoryListResult, BoardError> {
        let filter = fingerprint(&request)?;
        let workspace = self
            .workspace_show(WorkspaceShowRequest {
                workspace_id: request.workspace_id,
            })
            .await?
            .workspace;
        Ok(RepositoryListResult {
            page: self
                .metadata_page(
                    workspace.repositories,
                    request.page,
                    "repository_list",
                    filter,
                    |repository| format!("{repository:?}"),
                )
                .await?,
        })
    }
    pub async fn discovery_search(
        &self,
        request: DiscoverySearchRequest,
    ) -> Result<DiscoverySearchResult, BoardError> {
        let filter = fingerprint(&request)?;
        let after = self
            .metadata_after(&request.page, "discovery_search", &filter)
            .await?;
        let limit = usize::try_from(request.page.limit.get())
            .map_err(|_| BoardError::board_unavailable())?;
        let mut fleet = self
            .admission()?
            .fleet
            .read_snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut keys = fleet
            .discovery_page_keys(&request, &after, u64::from(request.page.limit.get()) + 1)
            .await
            .map_err(|error| admission_error(crate::FleetClientError::from(error).into()))?;
        let mut has_more = keys.len() > limit;
        keys.truncate(limit);
        let references: std::collections::BTreeMap<_, _> = keys
            .iter()
            .flat_map(|key| {
                std::iter::once(key.reference.clone())
                    .chain(std::iter::once(key.workspace.clone().into()))
                    .chain(key.board.clone().map(Into::into))
            })
            .map(|reference: RecordRef| (fleet_record_key(&reference), reference))
            .collect();
        let references: Vec<_> = references.into_values().collect();
        let stored = fleet
            .record_batch(&references)
            .await
            .map_err(|error| admission_error(crate::FleetClientError::from(error).into()))?;
        fleet
            .finish()
            .await
            .map_err(|error| admission_error(crate::FleetClientError::from(error).into()))?;
        if stored.len() != references.len() {
            return Err(BoardError::board_unavailable());
        }
        let mut bodies = DiscoveryBodies {
            stored: stored
                .into_iter()
                .map(|record| (fleet_record_key(record.reference()), record))
                .collect(),
            decoded: std::collections::BTreeMap::new(),
        };
        let mut hits = Vec::new();
        let mut bytes = 2_usize;
        for key in keys {
            let record = bodies.get(&key.reference)?;
            let Record::Workspace(workspace) = bodies.get(&key.workspace.into())? else {
                return Err(BoardError::board_unavailable());
            };
            let hit = match record {
                Record::Workspace(row) => DiscoveryHit::Workspace {
                    workspace: Box::new(row),
                },
                Record::Board(row) => DiscoveryHit::Board {
                    workspace: Box::new(workspace),
                    board: Box::new(row),
                },
                Record::DirectoryEntry(row) => {
                    let Record::Board(board) = bodies.get(&row.board.clone().into())? else {
                        return Err(BoardError::board_unavailable());
                    };
                    if row.envelope.scope.workspace != workspace.workspace_id
                        || board.workspace_id != workspace.workspace_id
                    {
                        return Err(BoardError::board_unavailable());
                    }
                    DiscoveryHit::Project {
                        workspace: Box::new(workspace),
                        board: Box::new(board),
                        project: Box::new(row),
                    }
                }
                _ => return Err(BoardError::board_unavailable()),
            };
            let size = serde_json::to_vec(&hit)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if hits.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(size).saturating_add(1);
            hits.push(hit);
        }
        Ok(DiscoverySearchResult {
            page: self
                .selected_metadata_page(
                    hits,
                    request.page,
                    "discovery_search",
                    filter,
                    has_more,
                    discovery_hit_key,
                )
                .await?,
        })
    }
}
fn discovery_hit_key(hit: &DiscoveryHit) -> String {
    match hit {
        DiscoveryHit::Workspace { workspace } => format!("0:{}", workspace.workspace_id.as_str()),
        DiscoveryHit::Board { board, .. } => format!("1:{}", board.board_id.as_str()),
        DiscoveryHit::Project { project, .. } => format!("2:{}", project.id.as_str()),
    }
}
struct DiscoveryBodies {
    stored: std::collections::BTreeMap<String, message_board_storage::StoredFleetRecord>,
    decoded: std::collections::BTreeMap<String, Record>,
}
impl DiscoveryBodies {
    fn get(&mut self, key: &RecordRef) -> Result<Record, BoardError> {
        let key = fleet_record_key(key);
        if let Some(record) = self.decoded.get(&key) {
            return Ok(record.clone());
        }
        let record = self
            .stored
            .remove(&key)
            .ok_or_else(BoardError::board_unavailable)?
            .decode()
            .map_err(|error| admission_error(crate::FleetClientError::from(error).into()))?;
        self.decoded.insert(key.clone(), record.clone());
        Ok(record)
    }
}

fn fleet_record_key(reference: &RecordRef) -> String {
    format!("{:?}:{}", reference.kind, reference.id.as_str())
}
