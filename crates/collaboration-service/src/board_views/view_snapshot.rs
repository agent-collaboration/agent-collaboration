//! One native snapshot supplies only the current-state operands used by each view.
use crate::ServiceIdentity;
use message_board::*;
use message_board_storage::{PendingTrailSelection, StoredProjectRecord};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) enum ViewReadSelection<'request> {
    Work { include_removed: bool },
    Card(&'request ContinuationCardRequest),
    Owner,
    Lead(&'request Identity),
}
pub(crate) struct ViewSnapshot {
    pub project: ProjectId,
    pub records: BTreeMap<(RecordKind, RecordId), Record>,
    pub creation: BTreeMap<(RecordKind, RecordId), (MergeTime, FeedPosition)>,
    pub(super) pending: super::pending_projection::PendingWork,
    pub recent_trail: Vec<StoredProjectRecord>,
    pub counts: BTreeMap<RecordKind, u64>,
    pub links: Vec<TaskLink>,
    pub as_of: FeedCursor,
    pub current_writer: CurrentWriter,
    pub writer_observation: Option<WriterObservation>,
    pub replicated_through: FeedCursor,
    pub source: CopySource,
}
impl ViewSnapshot {
    pub async fn load(
        identity: &ServiceIdentity,
        project: &ProjectId,
        selection: ViewReadSelection<'_>,
    ) -> Result<Option<Self>, BoardError> {
        let admission = identity
            .project_admission
            .as_ref()
            .ok_or_else(BoardError::board_unavailable)?;
        let store = admission
            .registry
            .read_project(project)
            .await
            .map_err(|error| {
                tracing::warn!(%error,"view store unavailable");
                BoardError::board_unavailable()
            })?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut snapshot = store.snapshot().await.map_err(|error| {
            tracing::warn!(%error, "view snapshot unavailable");
            BoardError::board_unavailable()
        })?;
        let (current_writer, writer_observation, source) = snapshot
            .view_context()
            .ok_or_else(BoardError::board_unavailable)?;
        let relevant = match selection {
            ViewReadSelection::Owner => {
                snapshot.has_owner_question().await.map_err(storage_error)?
            }
            ViewReadSelection::Lead(reader) => snapshot
                .holds_project_lead(reader)
                .await
                .map_err(storage_error)?,
            _ => true,
        };
        if !relevant {
            snapshot.finish().await.map_err(storage_error)?;
            return Ok(None);
        }
        let as_of = snapshot.high_water();
        let work = !matches!(selection, ViewReadSelection::Owner);
        let full_work = matches!(selection, ViewReadSelection::Work { .. });
        let card = matches!(selection, ViewReadSelection::Card(_));
        let include_removed = matches!(
            selection,
            ViewReadSelection::Work {
                include_removed: true
            }
        );
        let mut keys = Vec::new();
        let mut kinds = Vec::new();
        let mut global_work = work;
        let mut card_lead = false;
        let mut card_pair = false;
        let mut records = BTreeMap::new();
        let mut scoped_links = None;
        let mut card_workstream = None;
        if let ViewReadSelection::Card(request) = selection {
            card_lead = snapshot
                .holds_project_lead(&request.actor)
                .await
                .map_err(storage_error)?;
            if !card_lead && !matches!(request.subject, TrailSubject::Project) {
                let operands = snapshot
                    .card_work_operands(&request.subject)
                    .await
                    .map_err(storage_error)?;
                keys.extend(operands.records);
                scoped_links = Some(operands.links);
                card_workstream = operands.workstream;
                global_work = false;
            }
            let mut role_keys = snapshot
                .active_actor_subject_role_keys(
                    &request.actor,
                    &RoleSubject::Project(project.clone()),
                )
                .await
                .map_err(storage_error)?;
            if !card_lead && let Some(workstream) = &card_workstream {
                role_keys.extend(
                    snapshot
                        .active_actor_subject_role_keys(
                            &request.actor,
                            &RoleSubject::Workstream(workstream.clone()),
                        )
                        .await
                        .map_err(storage_error)?,
                );
            }
            if !role_keys.is_empty() {
                let rows = snapshot
                    .record_batch(&role_keys)
                    .await
                    .map_err(storage_error)?;
                if rows.len() != role_keys.len() {
                    return Err(BoardError::board_unavailable());
                }
                for stored in rows {
                    let record = stored.decode().map_err(storage_error)?;
                    let Record::RoleAssignment(role) = &record else {
                        return Err(BoardError::board_unavailable());
                    };
                    card_pair |= role.ended.is_none()
                        && role.holder.declared_identity().as_ref() == Some(&request.actor)
                        && card_workstream.as_ref().is_some_and(|workstream| {
                            role.subject() == RoleSubject::Workstream(workstream.clone())
                        })
                        && matches!(
                            role.role(),
                            AuthorityRole::Implementer | AuthorityRole::Verifier
                        );
                    let key = record.reference();
                    records.insert((key.kind, key.id), record);
                }
            }
        }
        let pending_inputs = match selection {
            ViewReadSelection::Work { .. } => message_board_storage::PendingTrailInputs::default(),
            ViewReadSelection::Owner => snapshot
                .pending_trail_inputs(PendingTrailSelection::Questions)
                .await
                .map_err(storage_error)?,
            ViewReadSelection::Lead(_) => snapshot
                .pending_trail_inputs(PendingTrailSelection::Handbacks)
                .await
                .map_err(storage_error)?,
            ViewReadSelection::Card(_) if card_lead => snapshot
                .pending_trail_inputs(PendingTrailSelection::Both)
                .await
                .map_err(storage_error)?,
            ViewReadSelection::Card(request) => snapshot
                .pending_card_trail_inputs(&request.subject, card_pair.then_some(&request.actor))
                .await
                .map_err(storage_error)?,
        };
        let pending = super::pending_projection::pending_work(pending_inputs)?;

        let links = if let Some(links) = scoped_links {
            links
        } else if global_work {
            snapshot.task_links().await.map_err(storage_error)?
        } else {
            Vec::new()
        };
        if global_work {
            kinds.extend([RecordKind::Workstream, RecordKind::Task]);
        }
        if full_work {
            kinds.extend([RecordKind::RoleAssignment, RecordKind::Subtask]);
        }
        for kind in kinds {
            let mut after = String::new();
            loop {
                let page = snapshot
                    .view_kind_page_keys(kind, &after, include_removed)
                    .await
                    .map_err(storage_error)?;
                let exhausted = page.len() <= 100;
                for key in page.into_iter().take(100) {
                    after = key.id.as_str().to_owned();
                    keys.push(key);
                }
                if exhausted {
                    break;
                }
            }
        }
        let counts = if card {
            snapshot.record_counts().await.map_err(storage_error)?
        } else {
            BTreeMap::new()
        };
        let mut recent_keys = Vec::new();
        if let ViewReadSelection::Card(request) = selection {
            keys.push(project.clone().into());
            if let Some(key) = snapshot.current_plan_key().await.map_err(storage_error)? {
                keys.push(key);
            }
            let mut after = String::new();
            loop {
                let page = snapshot
                    .card_intent_page_keys(&request.subject, &after)
                    .await
                    .map_err(storage_error)?;
                let exhausted = page.len() <= 100;
                for key in page.into_iter().take(100) {
                    after = key.id.as_str().to_owned();
                    keys.push(key);
                }
                if exhausted {
                    break;
                }
            }
            recent_keys = snapshot
                .recent_trail_keys(&request.subject)
                .await
                .map_err(storage_error)?;
        }
        keys.sort_by_key(|key| (key.kind, key.id.clone()));
        keys.dedup();
        let creation_keys: Vec<_> = keys
            .iter()
            .filter(|key| matches!(key.kind, RecordKind::Task | RecordKind::Workstream))
            .cloned()
            .collect();
        let creation = if creation_keys.is_empty() {
            BTreeMap::new()
        } else {
            snapshot
                .creation_metadata(&creation_keys)
                .await
                .map_err(storage_error)?
                .into_iter()
                .map(|key| {
                    (
                        (key.reference.kind, key.reference.id),
                        (key.time.into(), key.cursor.position),
                    )
                })
                .collect()
        };
        keys.retain(|key| !records.contains_key(&(key.kind, key.id.clone())));
        if !keys.is_empty() {
            let rows = snapshot.record_batch(&keys).await.map_err(storage_error)?;
            if rows.len() != keys.len() {
                return Err(BoardError::board_unavailable());
            }
            for row in rows {
                let record = row.decode().map_err(storage_error)?;
                let key = record.reference();
                records.insert((key.kind, key.id), record);
            }
        }
        let mut recent_by_id: BTreeMap<_, _> = if recent_keys.is_empty() {
            BTreeMap::new()
        } else {
            snapshot
                .record_batch(&recent_keys)
                .await
                .map_err(storage_error)?
                .into_iter()
                .map(|row| (row.reference().id.clone(), row))
                .collect()
        };
        let recent_trail = recent_keys
            .into_iter()
            .map(|key| {
                recent_by_id
                    .remove(&key.id)
                    .ok_or_else(BoardError::board_unavailable)
            })
            .collect::<Result<Vec<_>, _>>()?;
        snapshot.finish().await.map_err(storage_error)?;
        // The acknowledgement port can consult the registry. Release the read
        // gate first, so a queued terminal transition cannot deadlock this read.
        let acknowledged = match source {
            CopySource::ReadCopy {} => as_of,
            CopySource::Writer {} => {
                identity
                    .project_replication
                    .replicated_through(project, as_of.epoch)
                    .await?
            }
        };
        let replicated_through = if acknowledged.position > as_of.position {
            as_of
        } else {
            acknowledged
        };
        Ok(Some(Self {
            project: project.clone(),
            records,
            creation,
            pending,
            recent_trail,
            counts,
            links,
            as_of,
            current_writer,
            writer_observation,
            replicated_through,
            source,
        }))
    }
    pub fn tasks(&self) -> Vec<&Task> {
        self.records
            .values()
            .filter_map(|record| match record {
                Record::Task(row) => Some(row),
                _ => None,
            })
            .collect()
    }
    pub fn roles(&self) -> Vec<&RoleAssignment> {
        self.records
            .values()
            .filter_map(|record| match record {
                Record::RoleAssignment(row) if row.ended.is_none() => Some(row),
                _ => None,
            })
            .collect()
    }
    pub fn position(&self, reference: &RecordRef) -> Option<FeedCursor> {
        self.records
            .get(&(reference.kind, reference.id.clone()))
            .and_then(|record| record.envelope().position.project_cursor())
    }
}
fn storage_error(error: message_board_storage::ProjectStorageError) -> BoardError {
    tracing::warn!(%error,"view snapshot unavailable");
    BoardError::board_unavailable()
}
