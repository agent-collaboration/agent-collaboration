//! Subject bodies stay opaque until output selection, including its one byte-boundary candidate.
use super::{BoardOperations, admission_error};
use message_board::*;
use message_board_storage::{ProjectSnapshot, StoredFleetRecord, StoredProjectRecord};
use std::collections::BTreeMap;

enum SubjectRecord {
    Project(StoredProjectRecord),
    Fleet(StoredFleetRecord),
    Decoded(Box<Record>),
}
pub(super) struct ThreadSubjectBatch {
    records: BTreeMap<(RecordKind, RecordId), SubjectRecord>,
}
impl ThreadSubjectBatch {
    pub(super) fn take(
        &mut self,
        thread: &Thread,
        include_redacted: bool,
    ) -> Result<ThreadSubjectView, BoardError> {
        let reference: Option<RecordRef> = match &thread.about {
            ThreadSubject::Project { .. } => None,
            ThreadSubject::Workstream(id) => Some(id.clone().into()),
            ThreadSubject::Task(id) => Some(id.clone().into()),
            ThreadSubject::WorkTrailRow(id) => Some(id.clone().into()),
            ThreadSubject::WorkspaceAdr(id) => Some(id.clone().into()),
        };
        if let Some(reference) = &reference {
            let key = (reference.kind, reference.id.clone());
            let record = match self.records.remove(&key) {
                Some(SubjectRecord::Project(stored)) => Some(
                    stored
                        .decode()
                        .map_err(|error| admission_error(error.into()))?,
                ),
                Some(SubjectRecord::Fleet(stored)) => Some(
                    stored
                        .decode()
                        .map_err(|_| BoardError::board_unavailable())?,
                ),
                Some(SubjectRecord::Decoded(record)) => Some(*record),
                None => None,
            };
            if let Some(record) = record {
                self.records
                    .insert(key, SubjectRecord::Decoded(Box::new(record)));
            }
        }
        let lookup = |reference: RecordRef| match self.records.get(&(reference.kind, reference.id))
        {
            Some(SubjectRecord::Decoded(record)) => Some(record.as_ref()),
            _ => None,
        };
        let title = match &thread.about {
            ThreadSubject::Project { line } => SubjectTitle::Shown(line.as_str().to_owned()),
            ThreadSubject::Workstream(id) => match lookup(id.clone().into()) {
                Some(Record::Workstream(workstream)) => {
                    SubjectTitle::Shown(workstream.name.as_str().to_owned())
                }
                _ => return Err(BoardError::board_unavailable()),
            },
            ThreadSubject::Task(id) => match lookup(id.clone().into()) {
                Some(Record::Task(task)) => SubjectTitle::Shown(task.title.as_str().to_owned()),
                _ => return Err(BoardError::board_unavailable()),
            },
            ThreadSubject::WorkTrailRow(id) => match lookup(id.clone().into()) {
                Some(Record::WorkTrailRow(row)) if row.redacted && !include_redacted => {
                    SubjectTitle::Redacted
                }
                Some(Record::WorkTrailRow(row)) => SubjectTitle::Shown(
                    row.text
                        .as_str()
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                ),
                _ => return Err(BoardError::board_unavailable()),
            },
            ThreadSubject::WorkspaceAdr(id) => match lookup(id.clone().into()) {
                Some(Record::Adr(adr)) if adr.workspace == thread.envelope.scope.workspace => {
                    SubjectTitle::Shown(adr.title.as_str().to_owned())
                }
                None => SubjectTitle::Unavailable,
                _ => return Err(BoardError::board_unavailable()),
            },
        };
        Ok(ThreadSubjectView {
            about: thread.about.clone(),
            title,
        })
    }
}
impl BoardOperations<'_> {
    pub(super) async fn thread_subject_batch(
        &self,
        snapshot: &mut ProjectSnapshot,
        threads: &[ThreadId],
    ) -> Result<ThreadSubjectBatch, BoardError> {
        let keys = snapshot
            .thread_subject_operand_keys(threads)
            .await
            .map_err(|error| admission_error(error.into()))?;
        let (adr_keys, project_keys): (Vec<_>, Vec<_>) = keys
            .into_iter()
            .partition(|key| key.kind == RecordKind::Adr);
        let mut records = BTreeMap::new();
        if !project_keys.is_empty() {
            for stored in snapshot
                .record_batch(&project_keys)
                .await
                .map_err(|error| admission_error(error.into()))?
            {
                let key = stored.reference();
                records.insert((key.kind, key.id.clone()), SubjectRecord::Project(stored));
            }
        }
        if !adr_keys.is_empty() {
            let mut fleet = self
                .admission()?
                .fleet
                .read_snapshot()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
            for stored in fleet
                .record_batch(&adr_keys)
                .await
                .map_err(|_| BoardError::board_unavailable())?
            {
                let key = stored.reference();
                records.insert((key.kind, key.id.clone()), SubjectRecord::Fleet(stored));
            }
            fleet
                .finish()
                .await
                .map_err(|_| BoardError::board_unavailable())?;
        }
        Ok(ThreadSubjectBatch { records })
    }
}
