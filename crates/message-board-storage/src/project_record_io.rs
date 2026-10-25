//! Persist typed admitted records and their immutable feed snapshots in one transaction.

use message_board::{FeedCursor, Record, RecordKind, RecordPlacement, RecordRef};

use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};

pub(crate) fn record_kind_tag(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Workspace => "workspace",
        RecordKind::Board => "board",
        RecordKind::Adr => "adr",
        RecordKind::Project => "project",
        RecordKind::Workstream => "workstream",
        RecordKind::Task => "task",
        RecordKind::TaskLinkEvent => "taskLinkEvent",
        RecordKind::Subtask => "subtask",
        RecordKind::Thread => "thread",
        RecordKind::ThreadRefiled => "threadRefiled",
        RecordKind::Post => "post",
        RecordKind::RoleAssignment => "roleAssignment",
        RecordKind::OwnerGrant => "ownerGrant",
        RecordKind::StandDownAcknowledgement => "standDownAcknowledgement",
        RecordKind::Fact => "fact",
        RecordKind::PlanRevision => "planRevision",
        RecordKind::ReadMark => "readMark",
        RecordKind::NotifyIntent => "notifyIntent",
        RecordKind::SubscriptionMembership => "subscriptionMembership",
        RecordKind::Request => "request",
        RecordKind::WorkTrailRow => "workTrailRow",
        RecordKind::TrailRedaction => "trailRedaction",
    }
}

pub(crate) async fn read_record<TScope>(
    transaction: &mut TScope,
    project: &message_board::ProjectId,
    reference: &RecordRef,
) -> Result<Option<Record>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let mut rows = crate::project_record_batch::record_batch(
        transaction,
        project,
        std::slice::from_ref(reference),
    )
    .await?;
    if rows.len() > 1 {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field: "rowCardinality",
        });
    }
    rows.pop()
        .map(crate::StoredProjectRecord::decode)
        .transpose()
}

impl ProjectWriteTransaction {
    pub async fn record(
        &mut self,
        reference: &RecordRef,
    ) -> Result<Option<Record>, ProjectStorageError> {
        let record = read_record(&mut self.transaction, &self.project_id, reference).await?;
        if let Some(record) = &record {
            validate_project_record(record, &self.project_id)?;
        }
        Ok(record)
    }
    pub async fn append_record(
        &mut self,
        mut record: Record,
    ) -> Result<FeedCursor, ProjectStorageError> {
        let reference = record.reference();
        validate_project_record(&record, &self.project_id)?;
        if record.envelope().scope.project.as_ref() != Some(&self.project_id)
            || record.envelope().id != reference.id
            || matches!(
                reference.kind,
                RecordKind::Workspace | RecordKind::Board | RecordKind::Adr
            )
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "scope",
            });
        }
        if matches!(record, Record::ReadMark(_))
            || record.envelope().position == RecordPlacement::Unsequenced
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "position",
            });
        }
        if let Some(request) = &record.envelope().request
            && !matches!(record, Record::Request(_))
            && read_record(
                &mut self.transaction,
                &self.project_id,
                &request.clone().into(),
            )
            .await?
            .is_some()
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "decidedRequest",
            });
        }
        let existing = read_record(&mut self.transaction, &self.project_id, &reference).await?;
        if let Some(existing) = &existing {
            if existing.envelope().scope != record.envelope().scope {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "scope",
                });
            }
            if matches!(
                reference.kind,
                RecordKind::Post
                    | RecordKind::Fact
                    | RecordKind::WorkTrailRow
                    | RecordKind::TrailRedaction
                    | RecordKind::OwnerGrant
                    | RecordKind::StandDownAcknowledgement
                    | RecordKind::Request
            ) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "immutableContent",
                });
            }
        }
        let next = crate::project_epoch_reads::next_feed_cursor(&mut self.transaction).await?;
        if let Record::Request(ledger) = &record {
            crate::project_decision_effects::verify_decision(&mut self.transaction, ledger).await?;
        }
        let placement = RecordPlacement::Project(message_board::ProjectFeedEntry {
            cursor: next,
            origin: self.admission.origin(),
        });
        record.envelope_mut().position = placement.clone();
        if let Record::Post(row) = &mut record {
            row.position = placement;
        }
        let mode = if existing.is_some() {
            crate::project_record_codec::ProjectRowWrite::Update
        } else {
            crate::project_record_codec::ProjectRowWrite::Insert
        };
        let envelope = self.row_envelope(&record).await?;
        self.write_project_record(&record, &envelope, mode).await?;
        self.append_feed_row(&record, &envelope).await?;
        Ok(next)
    }
}

impl ProjectSnapshot {
    pub async fn record(
        &mut self,
        reference: &RecordRef,
    ) -> Result<Option<Record>, ProjectStorageError> {
        let record = read_record(&mut self.transaction, &self.project_id, reference).await?;
        if let Some(record) = &record {
            validate_project_record(record, &self.project_id)?;
        }
        if record.as_ref().is_some_and(|record| {
            record.envelope().scope.project.as_ref() != Some(&self.project_id)
        }) {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference.clone(),
                field: "scope",
            });
        }
        Ok(record)
    }
}

pub(crate) fn validate_project_record(
    record: &Record,
    project: &message_board::ProjectId,
) -> Result<(), ProjectStorageError> {
    let reference = record.reference();
    record
        .validate_bounds()
        .map_err(|error| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field: error.field,
        })?;
    if !matches!(
        (record, &record.envelope().position),
        (Record::ReadMark(_), RecordPlacement::Unsequenced)
    ) && (matches!(record, Record::ReadMark(_))
        || !matches!(record.envelope().position, RecordPlacement::Project(_)))
    {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "position",
        });
    }
    if record.envelope().id != reference.id {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "id",
        });
    }
    if record.envelope().scope.project.as_ref() != Some(project) {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "scope",
        });
    }
    let body_project = match record {
        Record::Project(row) => Some(&row.project_id),
        Record::Workstream(row) => Some(&row.project_id),
        Record::Task(row) => Some(&row.project_id),
        Record::WorkTrailRow(row) => Some(&row.project_id),
        Record::OwnerGrant(row) => match &row.grant {
            message_board::GrantKind::ReplaceLead { project, .. } => Some(project),
        },
        Record::ReadMark(row) => match &row.scope {
            message_board::ReadScope::Project { project_id } => Some(project_id),
            message_board::ReadScope::Thread { .. } => None,
        },
        Record::SubscriptionMembership(row) => match row.scope() {
            message_board::SubscriptionScope::Project { project_id } => Some(project_id),
            message_board::SubscriptionScope::Thread { .. } => None,
        },
        Record::RoleAssignment(row) => {
            if matches!(row.subject(), message_board::RoleSubject::Project(id) if id != *project) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "project",
                });
            }
            None
        }
        Record::Post(row) => {
            if row.position != record.envelope().position {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "position",
                });
            }
            if record.envelope().scope.board.as_ref() != Some(&row.board_id) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "board",
                });
            }
            if matches!(&row.placement, message_board::Placement::Project { project_id } if project_id != project)
            {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "project",
                });
            }
            Some(&row.project_id)
        }
        Record::Fact(fact) => {
            if let message_board::FactKind::Lifecycle(
                message_board::LifecycleFact::ProjectCreated { entry, .. },
            ) = &fact.kind
            {
                let directory = Record::DirectoryEntry((**entry).clone());
                if crate::fleet_record_codec::validate_fleet_record(&directory).is_err()
                    || entry.envelope.scope.workspace != fact.envelope.scope.workspace
                    || entry.envelope.scope.board != fact.envelope.scope.board
                {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: reference,
                        field: "scope",
                    });
                }
                Some(&entry.id)
            } else {
                None
            }
        }
        Record::Workspace(_) | Record::Board(_) | Record::Adr(_) | Record::DirectoryEntry(_) => {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "scope",
            });
        }
        Record::Subtask(_)
        | Record::TrailRedaction(_)
        | Record::Thread(_)
        | Record::NotifyIntent(_)
        | Record::Request(_)
        | Record::PlanRevision(_) => None,
        Record::StandDownAcknowledgement(_)
        | Record::TaskLinkEvent(_)
        | Record::ThreadRefiled(_) => None,
    };
    if body_project.is_some_and(|body| body != project) {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "project",
        });
    }
    Ok(())
}

/// Input constructors may normalize text; authoritative stored text must already be
/// canonical. Compare only trimming changes, preserving other valid roundtrip forms.
pub(crate) fn validate_canonical_project_text<TStored: serde::Serialize>(
    payload: &str,
    decoded: &TStored,
    reference: &RecordRef,
) -> Result<(), ProjectStorageError> {
    let original: serde_json::Value =
        serde_json::from_str(payload).map_err(|_| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field: "payload",
        })?;
    let canonical = serde_json::to_value(decoded)?;
    if let Some(field) = trimmed_stored_field(&original, &canonical, "payload") {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        });
    }
    Ok(())
}
fn trimmed_stored_field(
    original: &serde_json::Value,
    canonical: &serde_json::Value,
    field: &'static str,
) -> Option<&'static str> {
    match (original, canonical) {
        (serde_json::Value::String(original), serde_json::Value::String(canonical))
            if original != canonical && original.trim() == canonical =>
        {
            Some(field)
        }
        (serde_json::Value::Array(original), serde_json::Value::Array(canonical)) => original
            .iter()
            .zip(canonical)
            .find_map(|(original, canonical)| trimmed_stored_field(original, canonical, field)),
        (serde_json::Value::Object(original), serde_json::Value::Object(canonical)) => {
            canonical.iter().find_map(|(key, canonical)| {
                let original = original.get(key)?;
                let field = match key.as_str() {
                    "name" => "name",
                    "title" => "title",
                    "tags" => "tags",
                    "model" => "model",
                    "lineage" => "lineage",
                    "status" => "status",
                    "moveName" => "moveName",
                    "version" => "version",
                    "machine" => "machine",
                    "note" => "note",
                    _ => field,
                };
                trimmed_stored_field(original, canonical, field)
            })
        }
        _ => None,
    }
}
