//! Checked table batches defer domain-body decoding until a returned page needs it.
use crate::project_authority_rows::{StoredAuthorityRecord, authority_records};
use crate::project_conversation_rows::{StoredConversationRecord, conversation_records};
use crate::project_decision_rows::{StoredDecisionRecord, decision_records};
use crate::project_evidence_rows::{StoredEvidenceRecord, evidence_records};
use crate::project_reader_mark_io::{StoredReadMarkRow, reader_mark_records};
use crate::project_work_rows::{StoredWorkRecord, work_records};
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{ProjectId, Record, RecordKind, RecordRef};
use sqlx::Executor;
use sqlx_turso::Turso;
use std::collections::{BTreeMap, BTreeSet};

enum StoredProjectBody {
    Work(Box<StoredWorkRecord>),
    Conversation(Box<StoredConversationRecord>),
    Authority(Box<StoredAuthorityRecord>),
    Evidence(Box<StoredEvidenceRecord>),
    Decision(Box<StoredDecisionRecord>),
    ReadMark(Box<StoredReadMarkRow>),
}
impl StoredProjectBody {
    fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Work(row) => row.reference(),
            Self::Conversation(row) => row.reference(),
            Self::Authority(row) => row.reference(),
            Self::Evidence(row) => row.reference(),
            Self::Decision(row) => row.reference(),
            Self::ReadMark(row) => row.reference(),
        }
    }
    fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        match self {
            Self::Work(row) => row.decode(project),
            Self::Conversation(row) => row.decode(project),
            Self::Authority(row) => row.decode(project),
            Self::Evidence(row) => row.decode(project),
            Self::Decision(row) => row.decode(project),
            Self::ReadMark(row) => row.decode(project).map(Record::ReadMark),
        }
    }
}

pub struct StoredProjectRecord {
    project: ProjectId,
    reference: RecordRef,
    body: StoredProjectBody,
}
impl StoredProjectRecord {
    pub fn reference(&self) -> &RecordRef {
        &self.reference
    }
    pub fn decode(self) -> Result<Record, ProjectStorageError> {
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
        let record = self.body.decode(&self.project)?;
        if record.reference() != self.reference || record.envelope().id != self.reference.id {
            return Err(ProjectStorageError::InvalidRecord {
                record: self.reference,
                field: "id",
            });
        }
        crate::project_record_io::validate_project_record(&record, &self.project)?;
        Ok(record)
    }
}

pub(crate) async fn record_batch<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    references: &[RecordRef],
) -> Result<Vec<StoredProjectRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let mut groups: BTreeMap<RecordKind, BTreeSet<&str>> = BTreeMap::new();
    for reference in references {
        groups
            .entry(reference.kind)
            .or_default()
            .insert(reference.id.as_str());
    }
    let mut stored = Vec::new();
    for (kind, keys) in groups {
        let keys: Vec<_> = keys.into_iter().collect();
        let bodies: Vec<_> = match kind {
            RecordKind::Project
            | RecordKind::Workstream
            | RecordKind::Task
            | RecordKind::Subtask
            | RecordKind::TaskLinkEvent => work_records(transaction, project, kind, &keys)
                .await?
                .into_iter()
                .map(|row| StoredProjectBody::Work(Box::new(row)))
                .collect(),
            RecordKind::Thread | RecordKind::Post | RecordKind::ThreadRefiled => {
                conversation_records(transaction, project, kind, &keys)
                    .await?
                    .into_iter()
                    .map(|row| StoredProjectBody::Conversation(Box::new(row)))
                    .collect()
            }
            RecordKind::RoleAssignment
            | RecordKind::OwnerGrant
            | RecordKind::StandDownAcknowledgement => {
                authority_records(transaction, project, kind, &keys)
                    .await?
                    .into_iter()
                    .map(|row| StoredProjectBody::Authority(Box::new(row)))
                    .collect()
            }
            RecordKind::Fact
            | RecordKind::PlanRevision
            | RecordKind::WorkTrailRow
            | RecordKind::TrailRedaction => evidence_records(transaction, project, kind, &keys)
                .await?
                .into_iter()
                .map(|row| StoredProjectBody::Evidence(Box::new(row)))
                .collect(),
            RecordKind::Request | RecordKind::NotifyIntent | RecordKind::SubscriptionMembership => {
                decision_records(transaction, project, kind, &keys)
                    .await?
                    .into_iter()
                    .map(|row| StoredProjectBody::Decision(Box::new(row)))
                    .collect()
            }
            RecordKind::ReadMark => reader_mark_records(transaction, project, &keys)
                .await?
                .into_iter()
                .map(|row| StoredProjectBody::ReadMark(Box::new(row)))
                .collect(),
            RecordKind::Workspace | RecordKind::Board | RecordKind::Adr => {
                return Err(ProjectStorageError::InvalidFeed);
            }
        };
        let mut seen = BTreeSet::new();
        for body in bodies {
            let reference = body.reference()?;
            if reference.kind != kind
                || !keys.contains(&reference.id.as_str())
                || !seen.insert(reference.id.clone())
            {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "rowCardinality",
                });
            }
            stored.push(StoredProjectRecord {
                project: project.clone(),
                reference,
                body,
            });
        }
    }
    Ok(stored)
}

impl ProjectSnapshot {
    pub async fn record_batch(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<StoredProjectRecord>, ProjectStorageError> {
        record_batch(&mut self.transaction, &self.project_id, references).await
    }
    pub async fn record_keys(
        &mut self,
        kind: RecordKind,
        after: &str,
        limit: u64,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        crate::project_record_keys::record_keys(&mut self.transaction, kind, after, limit).await
    }
}
impl ProjectWriteTransaction {
    pub async fn record_batch(
        &mut self,
        references: &[RecordRef],
    ) -> Result<Vec<StoredProjectRecord>, ProjectStorageError> {
        record_batch(&mut self.transaction, &self.project_id, references).await
    }
}
