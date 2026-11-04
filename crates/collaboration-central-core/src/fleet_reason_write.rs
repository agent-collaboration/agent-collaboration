//! Insert one immutable refusal tree with IDs and ingestion time from its existing owner.
use crate::{
    fleet_identity_columns::IdentityColumns, fleet_reason_evidence::EvidenceEntry,
    fleet_reason_fields::EvidenceKind, fleet_reason_tree::EncodedReason, fleet_row_codec::corrupt,
    generated_fleet_reasons as queries,
};
use durable_steps::{StoreError, storage::TransactionScope};
use message_board::{Assignment, RequestId, RoleRequirement, RoleSubject};
use sqlx_core::{arguments::IntoArguments, database::Database, encode::Encode, types::Type};

pub(crate) async fn insert<S: TransactionScope>(
    scope: &mut S,
    request: &RequestId,
    nodes: Vec<EncodedReason>,
    now: runtime_primitives::SyncTime,
    ids: &mut crate::fleet_row_ids::FleetRowIds,
) -> Result<(), StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Encode<'v, S::Database>,
{
    for node in nodes {
        let mut fields = node.fields;
        let (record_kind, record_id) = match fields.record.take() {
            None => (None, None),
            Some(reference) => {
                let kind = serde_json::to_value(reference.kind).map_err(StoreError::backend)?;
                (
                    Some(
                        kind.as_str()
                            .ok_or_else(|| {
                                corrupt("reason.record_kind", "record kind is not a scalar")
                            })?
                            .into(),
                    ),
                    Some(reference.id.as_str().into()),
                )
            }
        };
        let (requirement_kind, requirement_role) = match fields.requirement.take() {
            None => (None, None),
            Some(RoleRequirement::Holds(role)) => {
                (Some("holds".into()), Some(role.as_str().into()))
            }
            Some(RoleRequirement::LeadOrWorkstreamImplementer) => {
                (Some("leadOrWorkstreamImplementer".into()), None)
            }
            Some(RoleRequirement::AnyProjectRole) => (Some("anyProjectRole".into()), None),
        };
        let (subject_kind, subject_id) = match fields.subject.take() {
            None => (None, None),
            Some(RoleSubject::Project(project)) => {
                (Some("project".into()), Some(project.as_str().into()))
            }
            Some(RoleSubject::Workstream(workstream)) => {
                (Some("workstream".into()), Some(workstream.as_str().into()))
            }
        };
        let actor = IdentityColumns::encode(fields.actor.take());
        let holder = IdentityColumns::encode(fields.holder.take());
        let target = IdentityColumns::encode(fields.target.take());
        let named = IdentityColumns::encode(fields.named_holder.take());
        let (seen_epoch, seen_position) = fields.last_seen.map_or((None, None), |cursor| {
            (
                Some(cursor.epoch.get().to_string()),
                Some(cursor.position.get().to_string()),
            )
        });
        let (evidence_kind, returned, total, digest, abridged) =
            fields
                .evidence
                .map_or((None, None, None, None, None), |header| {
                    (
                        Some(
                            match header.kind {
                                EvidenceKind::Task => "task",
                                EvidenceKind::Assignment => "assignment",
                            }
                            .into(),
                        ),
                        Some(header.returned.to_string()),
                        Some(header.total.to_string()),
                        Some(header.digest.as_str().into()),
                        Some(if header.abridged { "1" } else { "0" }.into()),
                    )
                });
        queries::insert_decision_reasons(
            scope,
            node.id.clone(),
            request.as_str().into(),
            node.parent,
            node.ordinal.to_string(),
            fields.group,
            fields.kind,
            record_kind,
            record_id,
            fields.name.map(|name| name.as_str().into()),
            fields.field,
            fields.revision.map(|revision| revision.get().to_string()),
            fields.retry.map(|retry| retry.get().to_string()),
            fields.workstream.map(|id| id.as_str().into()),
            fields.task.map(|id| id.as_str().into()),
            fields.thread.map(|id| id.as_str().into()),
            requirement_kind,
            requirement_role,
            subject_kind,
            subject_id,
            fields.role.map(|role| role.as_str().into()),
            actor.kind,
            actor.human,
            actor.service,
            actor.endpoint,
            actor.session,
            holder.kind,
            holder.human,
            holder.service,
            holder.endpoint,
            holder.session,
            target.kind,
            target.human,
            target.service,
            target.endpoint,
            target.session,
            named.kind,
            named.human,
            named.service,
            named.endpoint,
            named.session,
            seen_epoch,
            seen_position,
            evidence_kind,
            returned,
            total,
            digest,
            abridged,
            now.unix_millis().to_string(),
        )
        .await
        .map_err(StoreError::backend)?;
        for (ordinal, entry) in node.evidence.into_iter().enumerate() {
            let (kind, task, assignment_kind, project, workstream) = match entry {
                EvidenceEntry::Task(task) => ("task", Some(task.as_str().into()), None, None, None),
                EvidenceEntry::Assignment(assignment) => {
                    let (kind, project, workstream) = match assignment {
                        Assignment::Lead(project) => ("lead", Some(project.as_str().into()), None),
                        Assignment::Advisor(project) => {
                            ("advisor", Some(project.as_str().into()), None)
                        }
                        Assignment::Implementer(workstream) => {
                            ("implementer", None, Some(workstream.as_str().into()))
                        }
                        Assignment::Verifier(workstream) => {
                            ("verifier", None, Some(workstream.as_str().into()))
                        }
                        Assignment::Reviewer(workstream) => {
                            ("reviewer", None, Some(workstream.as_str().into()))
                        }
                        Assignment::Operator(workstream) => {
                            ("operator", None, Some(workstream.as_str().into()))
                        }
                    };
                    ("assignment", None, Some(kind.into()), project, workstream)
                }
            };
            queries::insert_decision_evidence(
                scope,
                ids.next()?.to_string(),
                node.id.clone(),
                ordinal.to_string(),
                kind.into(),
                task,
                assignment_kind,
                project,
                workstream,
                now.unix_millis().to_string(),
            )
            .await
            .map_err(StoreError::backend)?;
        }
    }
    Ok(())
}
