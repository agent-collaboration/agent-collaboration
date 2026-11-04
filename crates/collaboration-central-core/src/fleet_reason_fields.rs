//! Validated refusal payloads are independent of their immutable SQL node coordinates.
use crate::{
    fleet_column_codec::{canonical, decimal, optional_text, text, unsigned},
    fleet_identity_columns::IdentityColumns,
    fleet_row_codec::{check_millis, check_private_id, corrupt},
    generated_fleet_reasons::StoredDecisionReason,
};
use durable_steps::StoreError;
use message_board::{
    AuthorityRole, CanonicalHash, FeedCursor, Identity, PostCooldownSeconds, RecordRef, RequestId,
    ResourceName, RoleRequirement, RoleSubject, SubjectRevision, TaskId, ThreadId, WorkstreamId,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvidenceKind {
    Task,
    Assignment,
}
pub(crate) struct EvidenceHeader {
    pub kind: EvidenceKind,
    pub returned: u64,
    pub total: u64,
    pub digest: CanonicalHash,
    pub abridged: bool,
}
#[derive(Default)]
pub(crate) struct ReasonFields {
    pub group: String,
    pub kind: String,
    pub record: Option<RecordRef>,
    pub name: Option<ResourceName>,
    pub field: Option<String>,
    pub revision: Option<SubjectRevision>,
    pub retry: Option<PostCooldownSeconds>,
    pub workstream: Option<WorkstreamId>,
    pub task: Option<TaskId>,
    pub thread: Option<ThreadId>,
    pub requirement: Option<RoleRequirement>,
    pub subject: Option<RoleSubject>,
    pub role: Option<AuthorityRole>,
    pub actor: Option<Identity>,
    pub holder: Option<Identity>,
    pub target: Option<Identity>,
    pub named_holder: Option<Identity>,
    pub last_seen: Option<FeedCursor>,
    pub evidence: Option<EvidenceHeader>,
}
pub(crate) struct ReasonNode {
    pub id: String,
    pub request: RequestId,
    pub parent: Option<String>,
    pub ordinal: u64,
    pub fields: ReasonFields,
}
impl ReasonFields {
    pub(crate) fn tagged(group: &str, kind: &str) -> Self {
        Self {
            group: group.into(),
            kind: kind.into(),
            ..Self::default()
        }
    }
    pub(crate) fn active(&self, allowed: &[&str]) -> Result<(), StoreError> {
        for (name, present) in [
            ("record", self.record.is_some()),
            ("name", self.name.is_some()),
            ("field", self.field.is_some()),
            ("revision", self.revision.is_some()),
            ("retry", self.retry.is_some()),
            ("workstream", self.workstream.is_some()),
            ("task", self.task.is_some()),
            ("thread", self.thread.is_some()),
            ("requirement", self.requirement.is_some()),
            ("subject", self.subject.is_some()),
            ("role", self.role.is_some()),
            ("actor", self.actor.is_some()),
            ("holder", self.holder.is_some()),
            ("target", self.target.is_some()),
            ("named_holder", self.named_holder.is_some()),
            ("last_seen", self.last_seen.is_some()),
            ("evidence", self.evidence.is_some()),
        ] {
            if present && !allowed.contains(&name) {
                return Err(corrupt(
                    "reason.fields",
                    format!("inactive refusal field {name}"),
                ));
            }
        }
        Ok(())
    }
}
pub(crate) fn role(value: &str) -> Result<AuthorityRole, StoreError> {
    match value {
        "lead" => Ok(AuthorityRole::Lead),
        "advisor" => Ok(AuthorityRole::Advisor),
        "implementer" => Ok(AuthorityRole::Implementer),
        "verifier" => Ok(AuthorityRole::Verifier),
        "reviewer" => Ok(AuthorityRole::Reviewer),
        "operator" => Ok(AuthorityRole::Operator),
        _ => Err(corrupt("reason.role", "unknown authority role")),
    }
}
pub(crate) fn required<T>(value: Option<T>, field: &str) -> Result<T, StoreError> {
    value.ok_or_else(|| corrupt(field, "required refusal field is NULL"))
}

fn optional_number(
    value: Option<String>,
    class: Option<String>,
    field: &str,
) -> Result<Option<u64>, StoreError> {
    if value.is_none() && class.as_deref() == Some("null") {
        return Ok(None);
    }
    unsigned(value, class, field).map(Some)
}

pub(crate) fn decode_node(row: StoredDecisionReason) -> Result<ReasonNode, StoreError> {
    let id = text(row.reason_id, row.reason_id_class, "reason.id")?;
    check_private_id(&id, "reason.id")?;
    let request = canonical(
        text(row.request_id, row.request_id_class, "reason.request")?,
        "reason.request",
    )?;
    let parent = optional_text(
        row.parent_reason_id,
        row.parent_reason_id_class,
        "reason.parent_reason_id",
    )?;
    if let Some(parent) = &parent {
        check_private_id(parent, "reason.parent")?;
    }
    let ordinal = unsigned(row.ordinal, row.ordinal_class, "reason.ordinal")?;
    check_millis(row.created_at, row.created_at_class, "reason.created_at")?;
    check_millis(row.updated_at, row.updated_at_class, "reason.updated_at")?;
    let record = match (
        optional_text(row.record_kind, row.record_kind_class, "reason.record_kind")?,
        optional_text(row.record_id, row.record_id_class, "reason.record_id")?,
    ) {
        (None, None) => None,
        (Some(kind), Some(id)) => Some(RecordRef {
            kind: serde_json::from_value(serde_json::Value::String(kind))
                .map_err(|error| corrupt("reason.record_kind", error))?,
            id: canonical(id, "reason.record_id")?,
        }),
        _ => return Err(corrupt("reason.record", "partial reference")),
    };
    let role = optional_text(
        row.authority_role,
        row.authority_role_class,
        "reason.authority_role",
    )?
    .map(|value| role(&value))
    .transpose()?;
    let requirement = match (
        optional_text(
            row.requirement_kind,
            row.requirement_kind_class,
            "reason.requirement_kind",
        )?,
        optional_text(
            row.requirement_role,
            row.requirement_role_class,
            "reason.requirement_role",
        )?,
    ) {
        (None, None) => None,
        (Some(kind), value) => Some(match (kind.as_str(), value) {
            ("holds", Some(value)) => RoleRequirement::Holds(self::role(&value)?),
            ("leadOrWorkstreamImplementer", None) => RoleRequirement::LeadOrWorkstreamImplementer,
            ("anyProjectRole", None) => RoleRequirement::AnyProjectRole,
            _ => {
                return Err(corrupt(
                    "reason.requirement",
                    "unknown kind or inactive role",
                ));
            }
        }),
        _ => return Err(corrupt("reason.requirement", "partial requirement")),
    };
    let subject = match (
        optional_text(
            row.role_subject_kind,
            row.role_subject_kind_class,
            "reason.role_subject_kind",
        )?,
        optional_text(
            row.role_subject_id,
            row.role_subject_id_class,
            "reason.role_subject_id",
        )?,
    ) {
        (None, None) => None,
        (Some(kind), Some(id)) => Some(match kind.as_str() {
            "project" => RoleSubject::Project(canonical(id, "reason.subject")?),
            "workstream" => RoleSubject::Workstream(canonical(id, "reason.subject")?),
            _ => return Err(corrupt("reason.subject", "unknown subject kind")),
        }),
        _ => return Err(corrupt("reason.subject", "partial subject")),
    };
    let actor = IdentityColumns {
        kind: optional_text(row.actor_kind, row.actor_kind_class, "reason.actor_kind")?,
        human: optional_text(
            row.actor_human_id,
            row.actor_human_id_class,
            "reason.actor_human_id",
        )?,
        service: optional_text(
            row.actor_service_id,
            row.actor_service_id_class,
            "reason.actor_service_id",
        )?,
        endpoint: optional_text(
            row.actor_endpoint_id,
            row.actor_endpoint_id_class,
            "reason.actor_endpoint_id",
        )?,
        session: optional_text(
            row.actor_session_id,
            row.actor_session_id_class,
            "reason.actor_session_id",
        )?,
    }
    .decode("reason.actor")?;
    let holder = IdentityColumns {
        kind: optional_text(row.holder_kind, row.holder_kind_class, "reason.holder_kind")?,
        human: optional_text(
            row.holder_human_id,
            row.holder_human_id_class,
            "reason.holder_human_id",
        )?,
        service: optional_text(
            row.holder_service_id,
            row.holder_service_id_class,
            "reason.holder_service_id",
        )?,
        endpoint: optional_text(
            row.holder_endpoint_id,
            row.holder_endpoint_id_class,
            "reason.holder_endpoint_id",
        )?,
        session: optional_text(
            row.holder_session_id,
            row.holder_session_id_class,
            "reason.holder_session_id",
        )?,
    }
    .decode("reason.holder")?;
    let target = IdentityColumns {
        kind: optional_text(row.target_kind, row.target_kind_class, "reason.target_kind")?,
        human: optional_text(
            row.target_human_id,
            row.target_human_id_class,
            "reason.target_human_id",
        )?,
        service: optional_text(
            row.target_service_id,
            row.target_service_id_class,
            "reason.target_service_id",
        )?,
        endpoint: optional_text(
            row.target_endpoint_id,
            row.target_endpoint_id_class,
            "reason.target_endpoint_id",
        )?,
        session: optional_text(
            row.target_session_id,
            row.target_session_id_class,
            "reason.target_session_id",
        )?,
    }
    .decode("reason.target")?;
    let named_holder = IdentityColumns {
        kind: optional_text(
            row.named_holder_kind,
            row.named_holder_kind_class,
            "reason.named_holder_kind",
        )?,
        human: optional_text(
            row.named_holder_human_id,
            row.named_holder_human_id_class,
            "reason.named_holder_human_id",
        )?,
        service: optional_text(
            row.named_holder_service_id,
            row.named_holder_service_id_class,
            "reason.named_holder_service_id",
        )?,
        endpoint: optional_text(
            row.named_holder_endpoint_id,
            row.named_holder_endpoint_id_class,
            "reason.named_holder_endpoint_id",
        )?,
        session: optional_text(
            row.named_holder_session_id,
            row.named_holder_session_id_class,
            "reason.named_holder_session_id",
        )?,
    }
    .decode("reason.named_holder")?;
    let last_seen = match (
        optional_text(
            row.last_seen_epoch,
            row.last_seen_epoch_class,
            "reason.last_seen_epoch",
        )?,
        optional_text(
            row.last_seen_position,
            row.last_seen_position_class,
            "reason.last_seen_position",
        )?,
    ) {
        (None, None) => None,
        (Some(epoch), Some(position)) => Some(FeedCursor {
            epoch: decimal(&epoch, "reason.last_seen_epoch")?
                .try_into()
                .map_err(|error| corrupt("reason.last_seen_epoch", error))?,
            position: decimal(&position, "reason.last_seen_position")?
                .try_into()
                .map_err(|error| corrupt("reason.last_seen_position", error))?,
        }),
        _ => return Err(corrupt("reason.last_seen", "partial cursor")),
    };
    let returned = optional_number(
        row.evidence_returned_count,
        row.evidence_returned_count_class,
        "reason.evidence_returned",
    )?;
    let abridged = optional_number(
        row.evidence_abridged,
        row.evidence_abridged_class,
        "reason.evidence_abridged",
    )?
    .map(|value| match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(corrupt(
            "reason.evidence_abridged",
            "expected boolean INTEGER",
        )),
    })
    .transpose()?;
    let evidence = match (
        optional_text(
            row.evidence_kind,
            row.evidence_kind_class,
            "reason.evidence_kind",
        )?,
        returned,
        optional_text(
            row.evidence_total_count,
            row.evidence_total_count_class,
            "reason.evidence_total_count",
        )?,
        optional_text(
            row.evidence_digest,
            row.evidence_digest_class,
            "reason.evidence_digest",
        )?,
        abridged,
    ) {
        (None, None, None, None, None) => None,
        (Some(kind), Some(returned), Some(total), Some(digest), Some(abridged)) => {
            Some(EvidenceHeader {
                kind: match kind.as_str() {
                    "task" => EvidenceKind::Task,
                    "assignment" => EvidenceKind::Assignment,
                    _ => return Err(corrupt("reason.evidence_kind", "unknown evidence kind")),
                },
                returned,
                total: decimal(&total, "reason.evidence_total")?,
                digest: canonical(digest, "reason.evidence_digest")?,
                abridged,
            })
        }
        _ => return Err(corrupt("reason.evidence", "partial commitment header")),
    };
    let fields = ReasonFields {
        group: text(row.refusal_group, row.refusal_group_class, "reason.group")?,
        kind: text(row.refusal_kind, row.refusal_kind_class, "reason.kind")?,
        record,
        role,
        requirement,
        subject,
        actor,
        holder,
        target,
        named_holder,
        last_seen,
        evidence,
        name: optional_text(row.name, row.name_class, "reason.name")?
            .map(|value| canonical(value, "reason.name"))
            .transpose()?,
        field: optional_text(row.field, row.field_class, "reason.field")?,
        revision: optional_text(
            row.current_revision,
            row.current_revision_class,
            "reason.current_revision",
        )?
        .map(|value| {
            decimal(&value, "reason.revision").and_then(|value| {
                value
                    .try_into()
                    .map_err(|error| corrupt("reason.revision", error))
            })
        })
        .transpose()?,
        retry: optional_text(
            row.retry_after_seconds,
            row.retry_after_seconds_class,
            "reason.retry_after_seconds",
        )?
        .map(|value| {
            decimal(&value, "reason.retry").and_then(|value| {
                value
                    .try_into()
                    .map_err(|error| corrupt("reason.retry", error))
            })
        })
        .transpose()?,
        workstream: optional_text(
            row.workstream_id,
            row.workstream_id_class,
            "reason.workstream_id",
        )?
        .map(|value| canonical(value, "reason.workstream"))
        .transpose()?,
        task: optional_text(row.task_id, row.task_id_class, "reason.task_id")?
            .map(|value| canonical(value, "reason.task"))
            .transpose()?,
        thread: optional_text(row.thread_id, row.thread_id_class, "reason.thread_id")?
            .map(|value| canonical(value, "reason.thread"))
            .transpose()?,
    };
    Ok(ReasonNode {
        id,
        request,
        parent,
        ordinal,
        fields,
    })
}
