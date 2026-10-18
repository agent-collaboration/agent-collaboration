//! Persist typed admitted records and their immutable feed snapshots in one transaction.

use message_board::{
    FeedContent, FeedCursor, FeedEntry, FeedRead, Record, RecordKind, RecordPosition, RecordRef,
};
use sqlx_turso::TursoConnection;

use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};

pub(crate) fn record_kind_tag(kind: RecordKind) -> &'static str {
    match kind {
        RecordKind::Workspace => "workspace",
        RecordKind::Board => "board",
        RecordKind::Adr => "adr",
        RecordKind::ProcessDefinition => "processDefinition",
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
        RecordKind::Move => "move",
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

pub(crate) async fn read_record(
    connection: &mut TursoConnection,
    reference: &RecordRef,
) -> Result<Option<Record>, ProjectStorageError> {
    let kind = record_kind_tag(reference.kind);
    let id = reference.id.as_str();
    let payload = sqlx_turso::query_scalar!(
        r#"SELECT payload AS "payload!: String" FROM record_identities WHERE kind = ? AND id = ?"#,
        kind,
        id
    )
    .fetch_optional(&mut *connection)
    .await?;
    let record = payload
        .map(|payload| {
            tracing::trace!(target: "message_board_storage::decode", decode_kind = "record");
            let record: Record =
                serde_json::from_str(&payload).map_err(|_| ProjectStorageError::InvalidRecord {
                    record: reference.clone(),
                    field: invalid_record_payload_field(reference.kind, &payload),
                })?;
            if record.reference() != *reference || record.envelope().id != reference.id {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference.clone(),
                    field: "id",
                });
            }
            validate_canonical_project_text(&payload, &record, reference)?;
            Ok(record)
        })
        .transpose()?;
    match record {
        Some(record) => Ok(Some(project_record_projection(connection, record).await?)),
        None => Ok(None),
    }
}

/// Keep diagnostic field names without exposing corrupt stored values. Typed record
/// decoding remains authoritative; this classifies its failure at known domain fields.
pub(crate) fn invalid_record_payload_field(kind: RecordKind, payload: &str) -> &'static str {
    if kind != RecordKind::SubscriptionMembership {
        return "payload";
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
        return "payload";
    };
    if value.pointer("/record/policy/mode").is_none_or(|mode| {
        serde_json::from_value::<message_board::SubscriptionMode>(mode.clone()).is_err()
    }) {
        return "subscriptionMembership.policy.mode";
    }
    if value
        .pointer("/record/ended")
        .is_some_and(|ended| !ended.is_null())
        && value.pointer("/record/ended/at").is_none_or(|at| {
            serde_json::from_value::<chrono::DateTime<chrono::Utc>>(at.clone()).is_err()
        })
    {
        return "subscriptionMembership.ended.at";
    }
    "payload"
}

pub(crate) async fn project_record_projection(
    connection: &mut TursoConnection,
    mut record: Record,
) -> Result<Record, ProjectStorageError> {
    if let Record::WorkTrailRow(row) = &mut record {
        let id = row.row_id.as_str();
        row.redacted = sqlx_turso::query_scalar!(
            r#"SELECT redacted AS "redacted!: bool" FROM work_trail_rows WHERE row_id = ?"#,
            id
        )
        .fetch_one(connection)
        .await?;
    }
    Ok(record)
}

impl ProjectWriteTransaction {
    pub async fn record(
        &mut self,
        reference: &RecordRef,
    ) -> Result<Option<Record>, ProjectStorageError> {
        let record = read_record(&mut self.transaction, reference).await?;
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
                RecordKind::Workspace
                    | RecordKind::Board
                    | RecordKind::Adr
                    | RecordKind::ProcessDefinition
            )
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "scope",
            });
        }
        if matches!(record, Record::ReadMark(_))
            || record.envelope().position == RecordPosition::Unsequenced
        {
            return Err(ProjectStorageError::InvalidRecord {
                record: reference,
                field: "position",
            });
        }
        let existing = read_record(&mut self.transaction, &reference).await?;
        if let Some(existing) = &existing {
            if let (Record::Request(previous), Record::Request(next)) = (&existing, &record)
                && (!matches!(
                    previous.decision,
                    message_board::RequestDecision::Accepted { .. }
                ) || !matches!(next.decision, message_board::RequestDecision::Voided { .. })
                    || previous.canonical_hash != next.canonical_hash
                    || previous.observed_epoch != next.observed_epoch
                    || previous.accepted_in != next.accepted_in
                    || previous.resubmits != next.resubmits)
            {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "decision",
                });
            }
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
                    | RecordKind::Move
                    | RecordKind::WorkTrailRow
                    | RecordKind::TrailRedaction
                    | RecordKind::OwnerGrant
                    | RecordKind::StandDownAcknowledgement
            ) {
                return Err(ProjectStorageError::InvalidRecord {
                    record: reference,
                    field: "immutableContent",
                });
            }
        }
        let previous = crate::project_store::checkpoint(&mut self.transaction).await?;
        let next = FeedCursor {
            epoch: previous.epoch,
            position: previous
                .position
                .successor()
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
        };
        record.envelope_mut().position = RecordPosition::Project(next);
        if let Record::Post(row) = &mut record {
            row.position = RecordPosition::Project(next);
        }
        let kind = record_kind_tag(reference.kind);
        let id = reference.id.as_str();
        let payload = serde_json::to_string(&record)?;
        if existing.is_some() {
            sqlx_turso::query!(
                "UPDATE record_identities SET payload = ? WHERE kind = ? AND id = ?",
                payload,
                kind,
                id
            )
            .execute(&mut *self.transaction)
            .await?;
        } else {
            sqlx_turso::query!(
                "INSERT INTO record_identities(kind, id, payload) VALUES (?, ?, ?)",
                kind,
                id,
                payload
            )
            .execute(&mut *self.transaction)
            .await?;
        }
        self.write_record_relations(&record).await?;
        let envelope = record.envelope();
        let entry = FeedEntry {
            position: next.position,
            epoch: next.epoch,
            record: reference.clone(),
            request: envelope.request.clone(),
            router_time: envelope.router_time,
            actor: envelope.actor.clone(),
            content: FeedContent::Shown(Box::new(record)),
        };
        let encoded = serde_json::to_string(&entry)?;
        let position =
            i64::try_from(next.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let epoch =
            i64::try_from(next.epoch.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let request_id = entry.request.as_ref().map(|request| request.as_str());
        sqlx_turso::query!("INSERT INTO project_feed(position, epoch, record_kind, record_id, request_id, entry) VALUES (?, ?, ?, ?, ?, ?)", position, epoch, kind, id, request_id, encoded).execute(&mut *self.transaction).await?;
        sqlx_turso::query!(
            "UPDATE feed_checkpoint SET last_position = ? WHERE singleton = 1",
            position
        )
        .execute(&mut *self.transaction)
        .await?;
        Ok(next)
    }
}

impl ProjectSnapshot {
    pub async fn record(
        &mut self,
        reference: &RecordRef,
    ) -> Result<Option<Record>, ProjectStorageError> {
        let record = read_record(&mut self.transaction, reference).await?;
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
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn feed_after(
        &mut self,
        after: FeedCursor,
        limit: u64,
        include_redacted: bool,
    ) -> Result<FeedRead, ProjectStorageError> {
        let high_water = self.high_water();
        if after.epoch != high_water.epoch {
            return Ok(FeedRead::CursorSuperseded {
                resume_from: FeedCursor {
                    epoch: high_water.epoch,
                    position: message_board::FeedPosition::ZERO,
                },
            });
        }
        if limit == 0 || after.position > high_water.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let top = after
            .position
            .get()
            .saturating_add(limit)
            .min(high_water.position.get());
        let start =
            i64::try_from(after.position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let end = i64::try_from(top).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query!(r#"SELECT f.entry AS "entry!: String", CASE WHEN f.record_kind = 'workTrailRow' THEN t.redacted END AS "redacted: bool" FROM project_feed f LEFT JOIN work_trail_rows t ON t.row_id = f.record_id WHERE f.position > ? AND f.position <= ? ORDER BY f.position"#, start, end).fetch_all(&mut *self.transaction).await?;
        let mut entries = Vec::new();
        let mut expected = after.position.get();
        for row in rows {
            tracing::trace!(target: "message_board_storage::decode", decode_kind = "feed");
            let mut entry: FeedEntry =
                serde_json::from_str(&row.entry).map_err(|_| ProjectStorageError::InvalidFeed)?;
            validate_canonical_project_text(&row.entry, &entry, &entry.record)?;
            expected += 1;
            if entry.position.get() != expected || entry.epoch != high_water.epoch {
                return Err(ProjectStorageError::InvalidFeed);
            }
            if let FeedContent::Shown(record) = &mut entry.content {
                validate_project_record(record, &self.project_id)?;
                if record.reference() != entry.record
                    || record.envelope().position
                        != RecordPosition::Project(FeedCursor {
                            epoch: entry.epoch,
                            position: entry.position,
                        })
                    || record.envelope().scope.project.as_ref() != Some(&self.project_id)
                {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                let envelope = record.envelope();
                if envelope.actor != entry.actor
                    || envelope.request != entry.request
                    || envelope.router_time != entry.router_time
                {
                    return Err(ProjectStorageError::InvalidFeed);
                }
                if let Record::WorkTrailRow(trail) = record.as_mut() {
                    trail.redacted = row.redacted.ok_or(ProjectStorageError::InvalidFeed)?;
                    if trail.redacted && !include_redacted {
                        entry.content = FeedContent::Redacted;
                    }
                }
            }
            entries.push(entry);
        }
        if expected != top {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(FeedRead::Entries {
            entries,
            covered_through: FeedCursor {
                epoch: after.epoch,
                position: message_board::FeedPosition::try_from(top)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            },
            high_water,
        })
    }
}

pub(crate) fn validate_project_record(
    record: &Record,
    project: &message_board::ProjectId,
) -> Result<(), ProjectStorageError> {
    let reference = record.reference();
    if !matches!(
        (record, record.envelope().position),
        (Record::ReadMark(_), RecordPosition::Unsequenced)
    ) && (matches!(record, Record::ReadMark(_))
        || !matches!(record.envelope().position, RecordPosition::Project(_)))
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
        Record::Move(row) => Some(&row.project),
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
        Record::Workspace(_)
        | Record::Board(_)
        | Record::Adr(_)
        | Record::ProcessDefinition(_)
        | Record::DirectoryEntry(_) => {
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
