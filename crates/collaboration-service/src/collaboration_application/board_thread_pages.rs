//! Workspace scope, per-project keysets, key merge, then page-dependent validation.
use super::board_thread_subjects::ThreadSubjectBatch;
use super::{BoardOperations, admission_error, board_paging::fingerprint};
use collaboration_runtime::ProjectReadSnapshot;
use message_board::{
    BoardError, ImplementerHolder, OrchestratorHolder, Participant, ParticipantRole, Record,
    RecordRef, RecordedParticipantRole, ThreadId, ThreadListRequest, ThreadListResult,
    ThreadReadRecord,
};
use message_board_storage::{ParticipantBatch, StoredProjectRecord};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};

struct ProjectThreadPage {
    snapshot: ProjectReadSnapshot,
    records: BTreeMap<String, StoredProjectRecord>,
    subjects: ThreadSubjectBatch,
    participants: ParticipantBatch,
    decoded_participants: BTreeMap<ThreadId, Vec<Participant>>,
}

impl BoardOperations<'_> {
    pub async fn thread_list(
        &self,
        request: ThreadListRequest,
    ) -> Result<ThreadListResult, BoardError> {
        let filter = fingerprint(&request)?;
        let after = self
            .metadata_after(&request.page, "thread_list", &filter)
            .await?;
        let limit = usize::try_from(request.page.limit.get())
            .map_err(|_| BoardError::board_unavailable())?;
        let admission = self.admission()?;
        let projects = admission
            .fleet
            .workspace_projects(&request.workspace_id, &admission.registry)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut snapshots = Vec::new();
        for project in projects {
            let Some(store) = admission
                .registry
                .read_project(&project)
                .await
                .map_err(|error| admission_error(error.into()))?
            else {
                continue;
            };
            let mut snapshot = store
                .snapshot()
                .await
                .map_err(|error| admission_error(error.into()))?;
            let candidates = snapshot
                .thread_page_keys(
                    &after,
                    u64::from(request.page.limit.get()) + 1,
                    request.watched_only.then_some(&request.reader),
                )
                .await
                .map_err(|error| admission_error(error.into()))?;
            snapshots.push((snapshot, candidates));
        }
        let mut heap = BinaryHeap::new();
        for (project, (_, keys)) in snapshots.iter().enumerate() {
            if let Some(first) = keys.first() {
                heap.push(Reverse((first.clone(), project, 0_usize)));
            }
        }
        let mut merged = Vec::new();
        while merged.len() < limit {
            let Some(Reverse((thread, project, offset))) = heap.pop() else {
                break;
            };
            merged.push((project, thread));
            if let Some(next) = snapshots
                .get(project)
                .and_then(|(_, keys)| keys.get(offset + 1))
            {
                heap.push(Reverse((next.clone(), project, offset + 1)));
            }
        }
        let mut has_more = !heap.is_empty();
        let mut pages = Vec::new();
        for (project, (mut snapshot, _candidates)) in snapshots.into_iter().enumerate() {
            let selected: Vec<_> = merged
                .iter()
                .filter(|(index, _)| *index == project)
                .map(|(_, thread)| thread.clone())
                .collect();
            let records = if selected.is_empty() {
                Vec::new()
            } else {
                snapshot
                    .record_batch(
                        &selected
                            .iter()
                            .cloned()
                            .map(RecordRef::from)
                            .collect::<Vec<_>>(),
                    )
                    .await
                    .map_err(|error| admission_error(error.into()))?
            };
            let subjects = self.thread_subject_batch(&mut snapshot, &selected).await?;
            let participants = snapshot
                .participant_batch(&selected)
                .await
                .map_err(|error| admission_error(error.into()))?;
            pages.push(ProjectThreadPage {
                snapshot,
                records: records
                    .into_iter()
                    .map(|stored| (stored.reference().id.as_str().to_owned(), stored))
                    .collect(),
                subjects,
                participants,
                decoded_participants: BTreeMap::new(),
            });
        }
        let mut threads = Vec::new();
        let mut bytes = 2_usize;
        for (project, thread_id) in merged {
            let page = pages
                .get_mut(project)
                .ok_or_else(BoardError::board_unavailable)?;
            let record = page
                .records
                .remove(thread_id.as_str())
                .ok_or_else(BoardError::board_unavailable)?;
            let Record::Thread(mut thread) = record
                .decode()
                .map_err(|error| admission_error(error.into()))?
            else {
                return Err(BoardError::board_unavailable());
            };
            let subject = page.subjects.take(&thread, false)?;
            if thread.envelope.scope.workspace != request.workspace_id {
                return Err(admission_error(
                    message_board_storage::ProjectStorageError::InvalidRecord {
                        record: thread_id.into(),
                        field: "scope",
                    }
                    .into(),
                ));
            }
            let participants = page
                .participants
                .take(&thread_id)
                .map_err(|error| admission_error(error.into()))?;
            thread.orchestrator = participants
                .iter()
                .find(|participant| {
                    participant.is_open()
                        && participant.role
                            == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
                })
                .map(|participant| OrchestratorHolder {
                    identity: participant.identity.clone(),
                    last_seen_position: participant.last_seen_position,
                });
            thread.implementer = participants
                .iter()
                .find(|participant| {
                    participant.is_open()
                        && participant.role
                            == RecordedParticipantRole::Recorded(ParticipantRole::Implementer)
                })
                .map(|participant| ImplementerHolder {
                    identity: participant.identity.clone(),
                    last_seen_position: participant.last_seen_position,
                });
            page.decoded_participants.insert(thread_id, participants);
            let thread = ThreadReadRecord {
                subject,
                project_id: thread
                    .envelope
                    .scope
                    .project
                    .clone()
                    .ok_or_else(BoardError::board_unavailable)?,
                thread,
            };
            let size = serde_json::to_vec(&thread)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if threads.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(size).saturating_add(1);
            threads.push(thread);
        }
        for mut page in pages {
            page.snapshot
                .validate_participant_anchors(&page.decoded_participants)
                .await
                .map_err(|error| admission_error(error.into()))?;
            page.snapshot
                .finish()
                .await
                .map_err(|error| admission_error(error.into()))?;
        }
        Ok(ThreadListResult {
            page: self
                .selected_metadata_page(
                    threads,
                    request.page,
                    "thread_list",
                    filter,
                    has_more,
                    |thread| thread.thread.thread_id.as_str().to_owned(),
                )
                .await?,
        })
    }
}
