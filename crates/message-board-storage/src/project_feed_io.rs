//! Immutable feed snapshots retain the assigning epoch and separately stored provenance.
use crate::project_actor_rows::decode_actor;
use crate::project_record_codec::{
    ProjectRowEnvelope, decode_record_time, stored_bool, stored_counter, stored_json, stored_text,
};
use crate::{
    CursorValidity, ImportRowKind, ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction,
};
use message_board::{
    ActorOrigin, EpochNumber, FeedContent, FeedCursor, FeedEntry, FeedPosition, FeedRead,
    MachineLabel, ProjectFeedEntry, ProjectId, Record, RecordId, RecordPlacement, RecordRef,
    RecordTime, RecordedActor, RequestId, ServiceId,
};

struct FeedWindowRow {
    cursor_row_count: i64,
    cursor_epoch: Option<i64>,
    entry_id: Option<String>,
    epoch: Option<i64>,
    position: Option<i64>,
    record_kind: Option<String>,
    record_id: Option<String>,
    request_id: Option<String>,
    actor_id: Option<String>,
    actor_identity: Option<String>,
    origin_installation: Option<String>,
    origin_machine: Option<String>,
    router_time: Option<String>,
    time_basis: Option<String>,
    content: Option<String>,
    redacted: Option<i64>,
}

#[derive(Clone)]
pub(crate) struct ProjectFeedRow {
    pub(crate) entry_id: Option<String>,
    pub(crate) epoch: Option<i64>,
    pub(crate) position: Option<i64>,
    pub(crate) record_kind: Option<String>,
    pub(crate) record_id: Option<String>,
    pub(crate) request_id: Option<String>,
    pub(crate) actor_id: Option<String>,
    pub(crate) actor_identity: Option<String>,
    pub(crate) origin_installation: Option<String>,
    pub(crate) origin_machine: Option<String>,
    pub(crate) router_time: Option<String>,
    pub(crate) time_basis: Option<String>,
    pub(crate) content: Option<String>,
    pub(crate) redacted: Option<i64>,
}
impl FeedWindowRow {
    fn into_feed(self) -> ProjectFeedRow {
        ProjectFeedRow {
            entry_id: self.entry_id,
            epoch: self.epoch,
            position: self.position,
            record_kind: self.record_kind,
            record_id: self.record_id,
            request_id: self.request_id,
            actor_id: self.actor_id,
            actor_identity: self.actor_identity,
            origin_installation: self.origin_installation,
            origin_machine: self.origin_machine,
            router_time: self.router_time,
            time_basis: self.time_basis,
            content: self.content,
            redacted: self.redacted,
        }
    }
}

fn required<T>(value: Option<T>) -> Result<T, ProjectStorageError> {
    value.ok_or(ProjectStorageError::InvalidFeed)
}

impl ProjectFeedRow {
    pub(crate) fn decode(
        self,
        project: &ProjectId,
        include_redacted: bool,
    ) -> Result<Option<FeedEntry>, ProjectStorageError> {
        let Some(position) = self.position else {
            return if self.entry_id.is_none() {
                Ok(None)
            } else {
                Err(ProjectStorageError::InvalidFeed)
            };
        };
        tracing::trace!(target: "message_board_storage::decode", decode_kind = "feed");
        RecordId::try_from(required(self.entry_id)?)
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let reference = RecordRef {
            kind: serde_json::from_value(serde_json::Value::String(required(self.record_kind)?))
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
            id: RecordId::try_from(required(self.record_id)?)
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
        };
        let cursor = FeedCursor {
            epoch: stored_counter(required(self.epoch)?, &reference, "epoch")?,
            position: stored_counter(position, &reference, "position")?,
        };
        if cursor.position == FeedPosition::ZERO {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let actor = decode_actor(&required(self.actor_id)?, &required(self.actor_identity)?)?;
        let origin = ActorOrigin {
            installation: stored_text(
                required(self.origin_installation)?,
                &reference,
                "origin.installation",
                ServiceId::as_str,
            )?,
            machine: stored_text(
                required(self.origin_machine)?,
                &reference,
                "origin.machine",
                MachineLabel::as_str,
            )?,
        };
        let time = decode_record_time(self.router_time, &required(self.time_basis)?, &reference)?;
        let request = self
            .request_id
            .map(RequestId::try_from)
            .transpose()
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let mut content: FeedContent =
            stored_json(&required(self.content)?, &reference, "content")?;
        let FeedContent::Shown(record) = &mut content else {
            return Err(ProjectStorageError::InvalidFeed);
        };
        crate::project_record_io::validate_project_record(record, project)?;
        let envelope = record.envelope();
        if record.reference() != reference
            || envelope.actor != actor
            || envelope.request != request
            || envelope.time != time
            || envelope.position
                != RecordPlacement::Project(ProjectFeedEntry {
                    cursor,
                    origin: origin.clone(),
                })
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if matches!(time, RecordTime::Router(_))
            && matches!(&actor, RecordedActor::Router { installation, .. } if installation != &origin.installation)
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if let Record::WorkTrailRow(trail) = record.as_mut() {
            trail.redacted = stored_bool(required(self.redacted)?, &reference, "redacted")?;
            if trail.redacted && !include_redacted {
                content = FeedContent::Redacted;
            }
        }
        Ok(Some(FeedEntry {
            position: cursor.position,
            epoch: cursor.epoch,
            record: reference,
            request,
            time,
            actor,
            origin,
            content,
        }))
    }
}

impl ProjectSnapshot {
    #[tracing::instrument(skip_all, fields(project = %self.project_id.as_str()))]
    pub async fn feed_after(
        &mut self,
        after: FeedCursor,
        limit: u64,
        include_redacted: bool,
    ) -> Result<FeedRead, ProjectStorageError> {
        read_feed_after(
            &mut self.transaction,
            &self.project_id,
            self.high_water,
            after,
            limit,
            include_redacted,
        )
        .await
    }
}

impl ProjectWriteTransaction {
    /// The admission owner reads immutable activity inside its existing write transaction.
    pub async fn feed_after(
        &mut self,
        after: FeedCursor,
        limit: u64,
        include_redacted: bool,
    ) -> Result<FeedRead, ProjectStorageError> {
        let high_water = self.high_water().await?;
        read_feed_after(
            &mut self.transaction,
            &self.project_id,
            high_water,
            after,
            limit,
            include_redacted,
        )
        .await
    }
}

async fn read_feed_after<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    high_water: FeedCursor,
    after: FeedCursor,
    limit: u64,
    include_redacted: bool,
) -> Result<FeedRead, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    if limit == 0 {
        return Err(ProjectStorageError::InvalidCursor);
    }
    let top = after
        .position
        .get()
        .saturating_add(limit)
        .min(high_water.position.get());
    let start =
        i64::try_from(after.position.get()).map_err(|_| ProjectStorageError::InvalidCursor)?;
    let end = i64::try_from(top).map_err(|_| ProjectStorageError::InvalidCursor)?;
    // Validate the anchor in the same native statement as the page and its redaction projection.
    // The exceptional superseded/invalid path asks the shared cursor owner for its exact cutoff.
    let rows = sqlx_turso::query_file_as!(
        FeedWindowRow,
        "src/project_feed_window.sql",
        start,
        start,
        end
    )
    .fetch_all(&mut *transaction)
    .await?;
    let first = rows.first().ok_or(ProjectStorageError::InvalidFeed)?;
    if first.cursor_row_count > 1 {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let assigning_epoch = first
        .cursor_epoch
        .map(|epoch| stored_counter::<EpochNumber>(epoch, &project.clone().into(), "cursor.epoch"))
        .transpose()?;
    if after.position != FeedPosition::ZERO && assigning_epoch != Some(after.epoch) {
        match crate::project_epoch_reads::cursor_validity(&mut *transaction, after, high_water)
            .await?
        {
            CursorValidity::Valid => return Err(ProjectStorageError::InvalidFeed),
            CursorValidity::Superseded { resume_from } => {
                return Ok(FeedRead::CursorSuperseded { resume_from });
            }
            CursorValidity::Invalid => return Err(ProjectStorageError::InvalidCursor),
        }
    }
    let mut entries = Vec::new();
    let mut covered_through = after;
    let mut expected = after.position.get();
    for row in rows {
        if let Some(entry) = row.into_feed().decode(project, include_redacted)? {
            expected = expected
                .checked_add(1)
                .ok_or(ProjectStorageError::InvalidFeed)?;
            if entry.position.get() != expected {
                return Err(ProjectStorageError::InvalidFeed);
            }
            covered_through = FeedCursor {
                epoch: entry.epoch,
                position: entry.position,
            };
            entries.push(entry);
        }
    }
    if expected != top {
        return Err(ProjectStorageError::InvalidFeed);
    }
    Ok(FeedRead::Entries {
        entries,
        covered_through,
        high_water,
    })
}

impl ProjectWriteTransaction {
    pub(crate) async fn append_feed_row(
        &mut self,
        record: &Record,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let cursor = record
            .envelope()
            .position
            .project_cursor()
            .ok_or(ProjectStorageError::InvalidFeed)?;
        let epoch_key = cursor.epoch.get().to_string();
        let position_key = cursor.position.get().to_string();
        let entry_id = self
            .admission
            .row_key(ImportRowKind::FeedEntry, &[&epoch_key, &position_key])?;
        let entry_id = entry_id.as_str();
        let reference = record.reference();
        let kind = crate::project_record_io::record_kind_tag(reference.kind);
        let id = reference.id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let origin = self.admission.origin();
        let installation = origin.installation.as_str();
        let machine = origin.machine.as_str();
        let content = serde_json::to_string(&FeedContent::Shown(Box::new(record.clone())))?;
        sqlx_turso::query!(
            "INSERT INTO project_feed(entry_id, epoch, position, record_kind, record_id, request_id, actor_id, origin_installation, origin_machine, router_time, time_basis, content) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            entry_id, envelope.epoch, envelope.position, kind, id, request, actor, installation,
            machine, envelope.router_time, envelope.time_basis, content
        ).execute(&mut self.transaction).await?;
        Ok(())
    }
}
