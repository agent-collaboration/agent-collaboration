//! Project-local keysets, a prefix-preserving key merge, and batched lazy body validation.
use super::board_paging::{ProjectReadCursor, invalid_cursor};
use super::{BoardOperations, admission_error};
use message_board::*;
use message_board_storage::{
    FeedPageBounds, FeedPageDirection, FeedPageKey, PostPageFilter, PostPageScope, ProjectSnapshot,
    StoredFeedEntry,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) enum FeedPageSource<'request> {
    Posts {
        search: Option<(&'request str, SearchPostKind)>,
        active_only: bool,
    },
    Inbox {
        reader: &'request Identity,
        mode: InboxReadMode,
    },
}
struct ProjectFeedPage {
    project: ProjectId,
    snapshot: ProjectSnapshot,
    keys: VecDeque<FeedPageKey>,
    entries: BTreeMap<FeedPosition, StoredFeedEntry>,
}
pub(super) struct ProjectReadPage<TRecord> {
    pub records: Vec<TRecord>,
    pub ordering: PostOrdering,
    pub next: AggregateCursor,
    pub high_water: BTreeMap<ProjectId, FeedCursor>,
    pub unavailable: Vec<ProjectId>,
    pub next_cursor: Option<String>,
}
impl BoardOperations<'_> {
    pub(super) async fn project_feed_page<TRecord: Serialize>(
        &self,
        request: &PostListRequest,
        cursor: (&str, String, Option<ProjectReadCursor>),
        source: FeedPageSource<'_>,
        assemble: impl Fn(FeedEntry) -> Result<TRecord, BoardError>,
    ) -> Result<ProjectReadPage<TRecord>, BoardError> {
        let (operation, filter, prior) = cursor;
        let admission = self.admission()?;
        let selected: BTreeSet<_> = self
            .projects_for_post_scope(&request.scope)
            .await?
            .into_iter()
            .collect();
        let mut projects = selected.clone();
        let mut unavailable = Vec::new();
        if let FeedPageSource::Inbox { reader, .. } = source {
            for project in admission
                .registry
                .project_ids()
                .await
                .map_err(|error| admission_error(error.into()))?
            {
                if selected.contains(&project) {
                    continue;
                }
                let store = match admission.registry.existing_project(&project).await {
                    Ok(Some(store)) => store,
                    Ok(None) => continue,
                    Err(_) => {
                        unavailable.push(project);
                        continue;
                    }
                };
                if !matches!(
                    store.schema_state(),
                    message_board_storage::StoreSchemaState::Ready { .. }
                ) {
                    unavailable.push(project);
                    continue;
                }
                match store.has_active_inbox_watch(reader).await {
                    Ok(true) => {
                        projects.insert(project);
                    }
                    Ok(false) => {}
                    Err(_) => unavailable.push(project),
                }
            }
        }
        let descending = request.selection == PostSelection::Latest;
        let limit = usize::try_from(request.page.limit.get()).map_err(|_| invalid_cursor())?;
        let starting_cursor =
            prior
                .as_ref()
                .map(|cursor| &cursor.next)
                .or(match &request.selection {
                    PostSelection::AfterPosition {
                        after: ReadPosition::Aggregate { cursor },
                    }
                    | PostSelection::Range {
                        from: ReadPosition::Aggregate { cursor },
                        ..
                    } => Some(cursor),
                    _ => None,
                });
        // An unavailable/unreturned project must retain its supplied boundary.
        let mut next = starting_cursor.cloned().unwrap_or_default();
        if prior.is_none() && matches!(request.selection, PostSelection::Range { .. }) {
            // A range's `from` is inclusive; continuations are exclusive.
            for cursor in next.per_project.values_mut() {
                cursor.position = FeedPosition::try_from(cursor.position.get().saturating_sub(1))
                    .map_err(|_| invalid_cursor())?;
            }
        }
        let mut high_water = BTreeMap::new();
        let mut pages = Vec::new();
        let mut locations = BTreeMap::<WorkspaceId, bool>::new();
        let mut boards = BTreeMap::<BoardId, bool>::new();
        for project in projects {
            if matches!(
                source,
                FeedPageSource::Posts {
                    active_only: true,
                    ..
                }
            ) && !self
                .post_project_is_active(&project, &mut locations, &mut boards)
                .await?
            {
                continue;
            }
            let store = match admission.registry.existing_project(&project).await {
                Ok(Some(store))
                    if matches!(
                        store.schema_state(),
                        message_board_storage::StoreSchemaState::Ready { .. }
                    ) =>
                {
                    store
                }
                _ => {
                    unavailable.push(project);
                    continue;
                }
            };
            let mut snapshot = match store.snapshot().await {
                Ok(snapshot) => snapshot,
                Err(
                    message_board_storage::ProjectStorageError::Database(_)
                    | message_board_storage::ProjectStorageError::MoveAsidePending { .. },
                ) => {
                    unavailable.push(project);
                    continue;
                }
                Err(error) => return Err(admission_error(error.into())),
            };
            let head = snapshot.high_water();
            let upper = prior
                .as_ref()
                .and_then(|cursor| cursor.high_water.per_project.get(&project))
                .copied()
                .unwrap_or(head);
            if upper.epoch != head.epoch || upper.position > head.position {
                return Err(invalid_cursor());
            }
            let (initial, mut bounds) = page_bounds(request, prior.as_ref(), &project, upper)?;
            bounds.sweep = starting_cursor
                .filter(|cursor| !cursor.per_project.contains_key(&project))
                .and_then(|cursor| cursor.sweep.clone());
            // No returned position means this new scope must retain its sweep filter.
            if bounds.sweep.is_none() {
                next.per_project.entry(project.clone()).or_insert(initial);
            }
            high_water.insert(project.clone(), upper);
            if let FeedPageSource::Inbox {
                reader,
                mode: InboxReadMode::Unread,
            } = &source
            {
                let start = snapshot
                    .inbox_scan_start(reader, selected.contains(&project))
                    .await
                    .map_err(|error| admission_error(error.into()))?;
                if let Some(start) = start {
                    bounds.after = bounds.after.max(start.min(bounds.through));
                } else {
                    bounds.after = bounds.through;
                }
            }
            let keys = match &source {
                FeedPageSource::Posts { search, .. } => {
                    snapshot
                        .post_page_keys(
                            &bounds,
                            PostPageFilter {
                                scope: match &request.scope {
                                    PostListScope::Thread { thread } => {
                                        PostPageScope::Thread(&thread.thread)
                                    }
                                    PostListScope::AllWorkspaces if search.is_none() => {
                                        PostPageScope::Main
                                    }
                                    _ => PostPageScope::All,
                                },
                                search: *search,
                            },
                        )
                        .await
                }
                FeedPageSource::Inbox { reader, mode } => {
                    snapshot
                        .inbox_page_keys(&bounds, reader, *mode, selected.contains(&project))
                        .await
                }
            }
            .map_err(|error| admission_error(error.into()))?;
            let (scan_after, scan_through) = if keys.len() > limit {
                let last = keys
                    .last()
                    .ok_or_else(BoardError::board_unavailable)?
                    .cursor
                    .position;
                if descending {
                    (
                        FeedPosition::try_from(last.get().saturating_sub(1))
                            .map_err(|_| invalid_cursor())?,
                        bounds.through,
                    )
                } else {
                    (bounds.after, last)
                }
            } else {
                (bounds.after, bounds.through)
            };
            snapshot
                .validate_feed_range(scan_after, scan_through)
                .await
                .map_err(|error| admission_error(error.into()))?;
            pages.push(ProjectFeedPage {
                project,
                snapshot,
                keys: keys.into(),
                entries: BTreeMap::new(),
            });
        }
        let mut merged = Vec::new();
        while merged.len() < limit {
            let heads = pages.iter().enumerate().filter_map(|(index, page)| {
                page.keys
                    .front()
                    .map(|key| (key.router_time, &page.project, key.cursor.position, index))
            });
            let head = if descending { heads.max() } else { heads.min() };
            let Some((_, _, _, index)) = head else {
                break;
            };
            let key = pages
                .get_mut(index)
                .and_then(|page| page.keys.pop_front())
                .ok_or_else(BoardError::board_unavailable)?;
            merged.push((index, key));
        }
        let mut has_more = pages.iter().any(|page| !page.keys.is_empty());
        for (index, page) in pages.iter_mut().enumerate() {
            let keys: Vec<_> = merged
                .iter()
                .filter(|(owner, _)| *owner == index)
                .map(|(_, key)| key.clone())
                .collect();
            page.entries = page
                .snapshot
                .feed_entry_batch(&keys)
                .await
                .map_err(|error| admission_error(error.into()))?;
        }
        let mut records = Vec::new();
        let mut bytes = 2_usize;
        for (index, key) in merged {
            let page = pages
                .get_mut(index)
                .ok_or_else(BoardError::board_unavailable)?;
            let entry = page
                .entries
                .remove(&key.cursor.position)
                .ok_or_else(BoardError::board_unavailable)?
                .decode(false)
                .map_err(|error| admission_error(error.into()))?;
            let record = assemble(entry)?;
            let size = serde_json::to_vec(&record)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if records.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(size).saturating_add(1);
            next.per_project.insert(page.project.clone(), key.cursor);
            let returned = MergeKey {
                router_time: key.router_time,
                project_id: page.project.clone(),
                position: key.cursor.position,
            };
            next.sweep = Some(match next.sweep.take() {
                Some(previous) if descending => previous.min(returned),
                Some(previous) => previous.max(returned),
                None => returned,
            });
            records.push(record);
        }
        for page in pages {
            page.snapshot
                .finish()
                .await
                .map_err(|error| admission_error(error.into()))?;
        }
        let next_cursor = if has_more {
            Some(
                self.encode_read_cursor(&ProjectReadCursor {
                    operation: operation.to_owned(),
                    filter,
                    high_water: AggregateCursor {
                        per_project: high_water.clone(),
                        sweep: None,
                    },
                    next: next.clone(),
                })
                .await?,
            )
        } else {
            None
        };
        Ok(ProjectReadPage {
            records,
            ordering: if descending {
                PostOrdering::NewestFirst
            } else {
                PostOrdering::OldestFirst
            },
            next,
            high_water,
            unavailable,
            next_cursor,
        })
    }
    async fn post_project_is_active(
        &self,
        project: &ProjectId,
        workspaces: &mut BTreeMap<WorkspaceId, bool>,
        boards: &mut BTreeMap<BoardId, bool>,
    ) -> Result<bool, BoardError> {
        let admission = self.admission()?;
        let (workspace, board) = match admission
            .fleet
            .record(&project.clone().into())
            .await
            .map_err(|error| admission_error(error.into()))?
        {
            Some(Record::DirectoryEntry(directory)) => {
                (directory.envelope.scope.workspace, directory.board)
            }
            Some(_) => return Err(BoardError::board_unavailable()),
            None => {
                let store = admission
                    .registry
                    .existing_project(project)
                    .await
                    .map_err(|error| admission_error(error.into()))?
                    .ok_or_else(BoardError::board_unavailable)?;
                let mut snapshot = store
                    .snapshot()
                    .await
                    .map_err(|error| admission_error(error.into()))?;
                let request = snapshot
                    .project_creation_request()
                    .await
                    .map_err(|error| admission_error(error.into()))?
                    .ok_or_else(BoardError::board_unavailable)?;
                snapshot
                    .finish()
                    .await
                    .map_err(|error| admission_error(error.into()))?;
                let pending = admission
                    .fleet
                    .pending_request(&request)
                    .await
                    .map_err(|error| admission_error(error.into()))?
                    .ok_or_else(BoardError::board_unavailable)?;
                let fleet_admission::FleetOperation::ProjectDirectoryCreate {
                    project: pending_project,
                    board,
                    ..
                } = pending.request.operation
                else {
                    return Err(BoardError::board_unavailable());
                };
                if &pending_project != project {
                    return Err(BoardError::board_unavailable());
                }
                (pending.request.workspace, board)
            }
        };
        let workspace = &workspace;
        let board = &board;
        if !workspaces.contains_key(workspace) {
            let record = self.fleet_record(workspace.clone().into()).await?;
            workspaces.insert(
                workspace.clone(),
                matches!(record, Record::Workspace(row) if row.state == WorkspaceState::Active),
            );
        }
        if !boards.contains_key(board) {
            let record = self.fleet_record(board.clone().into()).await?;
            boards.insert(board.clone(), matches!(record, Record::Board(row) if matches!(row.state, BoardState::Open | BoardState::Closed)));
        }
        Ok(workspaces.get(workspace) == Some(&true) && boards.get(board) == Some(&true))
    }
}
fn page_bounds(
    request: &PostListRequest,
    prior: Option<&ProjectReadCursor>,
    project: &ProjectId,
    high: FeedCursor,
) -> Result<(FeedCursor, FeedPageBounds), BoardError> {
    let position_for = |position: &ReadPosition| -> Option<FeedCursor> {
        match position {
            ReadPosition::Project { cursor }
                if matches!(
                    request.scope,
                    PostListScope::Project { .. } | PostListScope::Thread { .. }
                ) =>
            {
                Some(*cursor)
            }
            ReadPosition::Aggregate { cursor } => cursor.per_project.get(project).copied(),
            _ => None,
        }
    };
    let zero = FeedCursor {
        epoch: high.epoch,
        position: FeedPosition::ZERO,
    };
    let initial = if let Some(prior) = prior {
        prior.next.per_project.get(project).copied().unwrap_or(zero)
    } else {
        match &request.selection {
            PostSelection::Latest => zero,
            PostSelection::AfterPosition { after } => position_for(after).unwrap_or(zero),
            PostSelection::Range { from, .. } => {
                let mut start = position_for(from).ok_or_else(invalid_cursor)?;
                start.position = FeedPosition::try_from(start.position.get().saturating_sub(1))
                    .map_err(|_| invalid_cursor())?;
                start
            }
        }
    };
    if initial.epoch != high.epoch || initial.position > high.position {
        return Err(invalid_cursor());
    }
    let descending = request.selection == PostSelection::Latest;
    let after = if descending {
        FeedPosition::ZERO
    } else {
        initial.position
    };
    let through = if descending && prior.is_some() && initial.position != FeedPosition::ZERO {
        FeedPosition::try_from(initial.position.get().saturating_sub(1))
            .map_err(|_| invalid_cursor())?
    } else if let PostSelection::Range { to, .. } = &request.selection {
        let end = position_for(to).ok_or_else(invalid_cursor)?;
        if end.epoch != high.epoch {
            return Err(invalid_cursor());
        }
        end.position.min(high.position)
    } else {
        high.position
    };
    if through < after {
        return Err(invalid_cursor());
    }
    Ok((
        initial,
        FeedPageBounds {
            after,
            through,
            direction: if descending {
                FeedPageDirection::Backward
            } else {
                FeedPageDirection::Forward
            },
            limit: u64::from(request.page.limit.get()) + 1,
            sweep: None,
        },
    ))
}
