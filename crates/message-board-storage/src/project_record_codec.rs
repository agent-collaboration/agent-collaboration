use crate::{ProjectStorageError, project_actor_rows::decode_actor};
use chrono::{DateTime, SecondsFormat, Utc};
use message_board::*;

pub(crate) struct ProjectRowEnvelope {
    pub actor_id: RecordId,
    pub request_id: Option<String>,
    pub epoch: i64,
    pub position: i64,
    pub router_time: Option<String>,
    pub time_basis: &'static str,
}

#[derive(Clone, Copy)]
pub(crate) enum ProjectRowWrite {
    Insert,
    Update,
}

pub(crate) fn require_one_row(
    changed: u64,
    reference: RecordRef,
) -> Result<(), ProjectStorageError> {
    if changed != 1 {
        return Err(ProjectStorageError::InvalidRecord {
            record: reference,
            field: "row",
        });
    }
    Ok(())
}

impl crate::ProjectWriteTransaction {
    pub(crate) async fn validate_record_scope(
        &mut self,
        envelope: &RecordEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let project = self.project_id.as_str();
        let row = sqlx_turso::query!(r#"SELECT workspace_id AS "workspace!: String",board_id AS "board!: String" FROM project_state WHERE project_id=?"#, project)
            .fetch_optional(&mut self.transaction).await?;
        if row.as_ref().is_none_or(|row| {
            row.workspace != envelope.scope.workspace.as_str()
                || envelope
                    .scope
                    .board
                    .as_ref()
                    .is_none_or(|board| board.as_str() != row.board)
                || envelope.scope.project.as_ref() != Some(&self.project_id)
        }) {
            return Err(ProjectStorageError::InvalidRecord {
                record: RecordRef {
                    kind: RecordKind::Project,
                    id: self.project_id.clone().into(),
                },
                field: "scope",
            });
        }
        Ok(())
    }

    pub(crate) async fn row_envelope(
        &mut self,
        record: &Record,
    ) -> Result<ProjectRowEnvelope, ProjectStorageError> {
        let reference = record.reference();
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        let envelope = record.envelope();
        if !matches!(record, Record::Project(_)) {
            self.validate_record_scope(envelope).await?;
        }
        let RecordPlacement::Project(entry) = &envelope.position else {
            return Err(invalid("position"));
        };
        if entry.cursor.epoch != self.admitted_epoch()? || entry.origin != self.admission.origin() {
            return Err(invalid("position"));
        }
        if !self.admission.admits_actor(&envelope.actor) {
            return Err(invalid("actor.installation"));
        }
        if !self.admission.admits_time(envelope.time) {
            return Err(invalid("time"));
        }
        let (router_time, time_basis) = encode_record_time(envelope.time);
        Ok(ProjectRowEnvelope {
            actor_id: self.intern_actor(&envelope.actor).await?,
            request_id: envelope
                .request
                .as_ref()
                .map(|request| request.as_str().to_owned()),
            epoch: i64::try_from(entry.cursor.epoch.get()).map_err(|_| invalid("epoch"))?,
            position: i64::try_from(entry.cursor.position.get())
                .map_err(|_| invalid("position"))?,
            router_time,
            time_basis,
        })
    }
}

pub(crate) struct StoredProjectEnvelope {
    pub record_id: String,
    pub workspace_id: String,
    pub board_id: String,
    pub project_id: String,
    pub actor_id: String,
    pub actor_identity: Option<String>,
    pub request_id: Option<String>,
    pub epoch: i64,
    pub position: i64,
    pub router_time: Option<String>,
    pub time_basis: String,
    pub feed_entry_id: Option<String>,
    pub feed_record_id: Option<String>,
    pub feed_record_kind: Option<String>,
    pub feed_actor_id: Option<String>,
    pub feed_request_id: Option<String>,
    pub feed_epoch: Option<i64>,
    pub feed_position: Option<i64>,
    pub feed_router_time: Option<String>,
    pub feed_time_basis: Option<String>,
    pub origin_installation: Option<String>,
    pub origin_machine: Option<String>,
}

impl StoredProjectEnvelope {
    pub(crate) fn decode(
        self,
        reference: &RecordRef,
        project: &ProjectId,
    ) -> Result<RecordEnvelope, ProjectStorageError> {
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        let id = RecordId::try_from(self.record_id).map_err(|_| invalid("id"))?;
        let project_id = ProjectId::try_from(self.project_id).map_err(|_| invalid("project"))?;
        if id != reference.id || project_id != *project {
            return Err(invalid("id"));
        }
        let entry_id = self.feed_entry_id.ok_or_else(|| invalid("feedEntry"))?;
        RecordId::try_from(entry_id).map_err(|_| invalid("feedEntry"))?;
        if self.feed_record_id.as_deref() != Some(id.as_str())
            || self.feed_record_kind.as_deref()
                != Some(crate::project_record_io::record_kind_tag(reference.kind))
            || self.feed_actor_id.as_deref() != Some(self.actor_id.as_str())
            || self.feed_request_id != self.request_id
            || self.feed_epoch != Some(self.epoch)
            || self.feed_position != Some(self.position)
            || self.feed_router_time != self.router_time
            || self.feed_time_basis.as_deref() != Some(self.time_basis.as_str())
        {
            return Err(invalid("feedEnvelope"));
        }
        let actor = decode_actor(
            &self.actor_id,
            &self.actor_identity.ok_or_else(|| invalid("actor"))?,
        )?;
        let time = decode_record_time(self.router_time, &self.time_basis, reference)?;
        let installation = ServiceId::try_from(
            self.origin_installation
                .ok_or_else(|| invalid("origin.installation"))?,
        )
        .map_err(|_| invalid("origin.installation"))?;
        let stored_machine = self
            .origin_machine
            .ok_or_else(|| invalid("origin.machine"))?;
        let machine = MachineLabel::try_from(stored_machine.clone())
            .map_err(|_| invalid("origin.machine"))?;
        if machine.as_str() != stored_machine {
            return Err(invalid("origin.machine"));
        }
        if matches!(time, RecordTime::Router(_))
            && matches!(&actor, RecordedActor::Router { installation: own, .. } if own != &installation)
        {
            return Err(invalid("actor.installation"));
        }
        let epoch = EpochNumber::try_from(u64::try_from(self.epoch).map_err(|_| invalid("epoch"))?)
            .map_err(|_| invalid("epoch"))?;
        let position =
            FeedPosition::try_from(u64::try_from(self.position).map_err(|_| invalid("position"))?)
                .map_err(|_| invalid("position"))?;
        Ok(RecordEnvelope {
            id,
            scope: RecordScope {
                workspace: WorkspaceId::try_from(self.workspace_id)
                    .map_err(|_| invalid("workspace"))?,
                board: Some(BoardId::try_from(self.board_id).map_err(|_| invalid("board"))?),
                project: Some(project_id),
            },
            actor,
            request: self
                .request_id
                .map(RequestId::try_from)
                .transpose()
                .map_err(|_| invalid("request"))?,
            position: RecordPlacement::Project(ProjectFeedEntry {
                cursor: FeedCursor { epoch, position },
                origin: ActorOrigin {
                    installation,
                    machine,
                },
            }),
            time,
        })
    }
}

pub(crate) fn encode_record_time(time: RecordTime) -> (Option<String>, &'static str) {
    match time {
        RecordTime::Router(time) => (Some(timestamp(time)), "router"),
        RecordTime::LegacyUnknown => (None, "legacy_unknown"),
    }
}

pub(crate) fn decode_record_time(
    value: Option<String>,
    basis: &str,
    reference: &RecordRef,
) -> Result<RecordTime, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field: "time",
    };
    match (basis, value) {
        ("router", Some(value)) => {
            let parsed: DateTime<Utc> = value.parse().map_err(|_| invalid())?;
            if timestamp(parsed) != value {
                return Err(invalid());
            }
            Ok(RecordTime::Router(parsed))
        }
        ("legacy_unknown", None) => Ok(RecordTime::LegacyUnknown),
        _ => Err(invalid()),
    }
}

pub(crate) fn timestamp(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

pub(crate) fn encode_merge_sweep(sweep: &MergeKey) -> Result<String, ProjectStorageError> {
    let mut encoded = serde_json::to_value(sweep)?;
    if let MergeTime::Known(time) = sweep.time {
        // Match the fixed-precision stored form, including an exact-second tie.
        let value = encoded
            .get_mut("time")
            .and_then(|time| time.get_mut("value"))
            .ok_or(ProjectStorageError::InvalidFeed)?;
        *value = serde_json::Value::String(timestamp(time));
    }
    Ok(serde_json::to_string(&encoded)?)
}

pub(crate) fn task_link_kind_tag(kind: TaskLinkKind) -> &'static str {
    match kind {
        TaskLinkKind::BlockedBy => "blockedBy",
        TaskLinkKind::RelatedTo => "relatedTo",
    }
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", content = "recipient", rename_all = "camelCase")]
enum LogicalIntentRecipient {
    Actor(Identity),
    Role {
        subject: RoleSubject,
        role: AuthorityRole,
    },
}

pub(crate) fn intent_recipient_key(
    recipient: &IntentRecipient,
    reference: &RecordRef,
) -> Result<String, ProjectStorageError> {
    let logical = match recipient {
        IntentRecipient::Resolved(recipient) => {
            LogicalIntentRecipient::Actor(recipient.actor.declared_identity().ok_or_else(|| {
                ProjectStorageError::InvalidRecord {
                    record: reference.clone(),
                    field: "recipient.actor",
                }
            })?)
        }
        IntentRecipient::NoHolder { subject, role, .. } => LogicalIntentRecipient::Role {
            subject: subject.clone(),
            role: *role,
        },
    };
    Ok(serde_json::to_string(&logical)?)
}

pub(crate) fn stored_json<T: serde::de::DeserializeOwned + serde::Serialize>(
    value: &str,
    reference: &RecordRef,
    field: &'static str,
) -> Result<T, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field,
    };
    let decoded = serde_json::from_str(value).map_err(|_| invalid())?;
    crate::project_record_io::validate_canonical_project_text(value, &decoded, reference)?;
    Ok(decoded)
}

pub(crate) fn stored_text<T: TryFrom<String>>(
    value: String,
    reference: &RecordRef,
    field: &'static str,
    canonical: fn(&T) -> &str,
) -> Result<T, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field,
    };
    let decoded = T::try_from(value.clone()).map_err(|_| invalid())?;
    if canonical(&decoded) != value {
        return Err(invalid());
    }
    Ok(decoded)
}

pub(crate) fn stored_counter<T: TryFrom<u64>>(
    value: i64,
    reference: &RecordRef,
    field: &'static str,
) -> Result<T, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field,
    };
    T::try_from(u64::try_from(value).map_err(|_| invalid())?).map_err(|_| invalid())
}

pub(crate) fn stored_tag<T: serde::de::DeserializeOwned>(
    value: String,
    reference: &RecordRef,
    field: &'static str,
) -> Result<T, ProjectStorageError> {
    serde_json::from_value(serde_json::Value::String(value)).map_err(|_| {
        ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        }
    })
}

pub(crate) fn stored_timestamp(
    value: String,
    reference: &RecordRef,
    field: &'static str,
) -> Result<DateTime<Utc>, ProjectStorageError> {
    let invalid = || ProjectStorageError::InvalidRecord {
        record: reference.clone(),
        field,
    };
    let time = value.parse().map_err(|_| invalid())?;
    if timestamp(time) != value {
        return Err(invalid());
    }
    Ok(time)
}

pub(crate) fn stored_bool(
    value: i64,
    reference: &RecordRef,
    field: &'static str,
) -> Result<bool, ProjectStorageError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        }),
    }
}

macro_rules! stored_project_row {
    ($row:ident, $body:ident { $($field:ident: $value:ty),* $(,)? }) => {
        pub(crate) struct $row {
            record_id: String,
            workspace_id: String,
            board_id: String,
            project_id: String,
            actor_id: String,
            actor_identity: Option<String>,
            request_id: Option<String>,
            epoch: i64,
            position: i64,
            router_time: Option<String>,
            time_basis: String,
            feed_entry_id: Option<String>,
            feed_record_id: Option<String>,
            feed_record_kind: Option<String>,
            feed_actor_id: Option<String>,
            feed_request_id: Option<String>,
            feed_epoch: Option<i64>,
            feed_position: Option<i64>,
            feed_router_time: Option<String>,
            feed_time_basis: Option<String>,
            origin_installation: Option<String>,
            origin_machine: Option<String>,
            $($field: $value),*
        }
        pub(crate) struct $body { $($field: $value),* }
        impl $row {
            fn reference(&self, kind: message_board::RecordKind) -> Result<message_board::RecordRef, crate::ProjectStorageError> {
                Ok(message_board::RecordRef {
                    kind,
                    id: message_board::RecordId::try_from(self.record_id.clone())
                        .map_err(|_| crate::ProjectStorageError::InvalidFeed)?,
                })
            }
            fn into_parts(self) -> (crate::project_record_codec::StoredProjectEnvelope, $body) {
                (crate::project_record_codec::StoredProjectEnvelope {
                    record_id: self.record_id, workspace_id: self.workspace_id,
                    board_id: self.board_id, project_id: self.project_id,
                    actor_id: self.actor_id, actor_identity: self.actor_identity,
                    request_id: self.request_id, epoch: self.epoch, position: self.position,
                    router_time: self.router_time, time_basis: self.time_basis,
                    feed_entry_id: self.feed_entry_id, feed_record_id: self.feed_record_id,
                    feed_record_kind: self.feed_record_kind, feed_actor_id: self.feed_actor_id,
                    feed_request_id: self.feed_request_id, feed_epoch: self.feed_epoch,
                    feed_position: self.feed_position, feed_router_time: self.feed_router_time,
                    feed_time_basis: self.feed_time_basis,
                    origin_installation: self.origin_installation, origin_machine: self.origin_machine,
                }, $body { $($field: self.$field),* })
            }
        }
    };
}
pub(crate) use stored_project_row;
