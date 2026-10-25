//! Authority rows decode explicit subjects and interned actor references.
use crate::ProjectStorageError;
use crate::project_actor_rows::decode_actor;
use crate::project_record_codec::{
    stored_counter, stored_json, stored_project_row, stored_tag, stored_text,
};
use message_board::*;
use sqlx::Executor;
use sqlx_turso::Turso;

stored_project_row!(StoredRole, RoleBody {
    project_subject: Option<String>, workstream_subject: Option<String>, role: String,
    holder: String, holder_identity: Option<String>, granted_by: String, granted_identity: Option<String>,
    model: Option<String>, lineage: Option<String>, ended: Option<String>, owner_grant: Option<String>,
});
stored_project_row!(StoredGrant, GrantBody {
    grant_kind: String, grant_project_id: String, new_holder: String, holder_identity: Option<String>,
    reason: String, provenance_kind: String, relayed_by: String, relayed_identity: Option<String>,
});
stored_project_row!(
    StoredStandDownAck,
    StandDownAckBody {
        assignment_id: String,
        through_epoch: i64,
        through_position: i64
    }
);

pub(crate) enum StoredAuthorityRecord {
    Role(StoredRole),
    Grant(StoredGrant),
    StandDownAck(StoredStandDownAck),
}
impl StoredAuthorityRecord {
    pub(crate) fn reference(&self) -> Result<RecordRef, ProjectStorageError> {
        match self {
            Self::Role(row) => row.reference(RecordKind::RoleAssignment),
            Self::Grant(row) => row.reference(RecordKind::OwnerGrant),
            Self::StandDownAck(row) => row.reference(RecordKind::StandDownAcknowledgement),
        }
    }
    pub(crate) fn decode(self, project: &ProjectId) -> Result<Record, ProjectStorageError> {
        let reference = self.reference()?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: reference.clone(),
            field,
        };
        Ok(match self {
            Self::Role(row) => {
                let (raw, body) = row.into_parts();
                let subject = match (body.project_subject, body.workstream_subject) {
                    (Some(id), None) if id == project.as_str() => {
                        RoleSubject::Project(project.clone())
                    }
                    (None, Some(id)) => RoleSubject::Workstream(stored_text(
                        id,
                        &reference,
                        "subject",
                        WorkstreamId::as_str,
                    )?),
                    _ => return Err(invalid("subject")),
                };
                Record::RoleAssignment(
                    RoleAssignment::new(RoleAssignmentProps {
                        envelope: raw.decode(&reference, project)?,
                        assignment_id: RoleAssignmentId::try_from(reference.id.as_str().to_owned())
                            .map_err(|_| invalid("id"))?,
                        subject,
                        role: stored_tag(body.role, &reference, "role")?,
                        holder: decode_actor(
                            &body.holder,
                            &body.holder_identity.ok_or_else(|| invalid("holder"))?,
                        )?,
                        granted_by: decode_actor(
                            &body.granted_by,
                            &body.granted_identity.ok_or_else(|| invalid("grantedBy"))?,
                        )?,
                        model: body
                            .model
                            .map(|value| {
                                stored_text(value, &reference, "model", ResourceName::as_str)
                            })
                            .transpose()?,
                        lineage: body
                            .lineage
                            .map(|value| {
                                stored_text(value, &reference, "lineage", ResourceName::as_str)
                            })
                            .transpose()?,
                        ended: body
                            .ended
                            .map(|value| stored_json(&value, &reference, "ended"))
                            .transpose()?,
                        owner_grant: body
                            .owner_grant
                            .map(|value| {
                                stored_text(value, &reference, "ownerGrant", OwnerGrantId::as_str)
                            })
                            .transpose()?,
                    })
                    .map_err(|_| invalid("assignment"))?,
                )
            }
            Self::Grant(row) => {
                let (raw, body) = row.into_parts();
                if body.grant_kind != "replaceLead"
                    || body.grant_project_id != project.as_str()
                    || body.provenance_kind != "relayed"
                {
                    return Err(invalid("grant"));
                }
                Record::OwnerGrant(OwnerGrant {
                    envelope: raw.decode(&reference, project)?,
                    id: OwnerGrantId::try_from(reference.id.as_str().to_owned())
                        .map_err(|_| invalid("id"))?,
                    grant: GrantKind::ReplaceLead {
                        project: project.clone(),
                        new_holder: decode_actor(
                            &body.new_holder,
                            &body.holder_identity.ok_or_else(|| invalid("newHolder"))?,
                        )?,
                        reason: body.reason,
                    },
                    provenance: OwnerWordsProvenance::Relayed {
                        by: decode_actor(
                            &body.relayed_by,
                            &body.relayed_identity.ok_or_else(|| invalid("relayedBy"))?,
                        )?,
                    },
                })
            }
            Self::StandDownAck(row) => {
                let (raw, body) = row.into_parts();
                Record::StandDownAcknowledgement(StandDownAcknowledgement {
                    envelope: raw.decode(&reference, project)?,
                    assignment: stored_text(
                        body.assignment_id,
                        &reference,
                        "assignment",
                        RoleAssignmentId::as_str,
                    )?,
                    read_through: FeedCursor {
                        epoch: stored_counter(body.through_epoch, &reference, "through")?,
                        position: stored_counter(body.through_position, &reference, "through")?,
                    },
                })
            }
        })
    }
}

pub(crate) async fn authority_records<TScope>(
    transaction: &mut TScope,
    project: &ProjectId,
    kind: RecordKind,
    keys: &[&str],
) -> Result<Vec<StoredAuthorityRecord>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    let keys = serde_json::to_string(keys)?;
    Ok(match kind {
        RecordKind::RoleAssignment => {
            sqlx_turso::query_file_as!(StoredRole, "src/project_role_batch.sql", project, keys)
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(StoredAuthorityRecord::Role)
                .collect()
        }
        RecordKind::OwnerGrant => {
            sqlx_turso::query_file_as!(StoredGrant, "src/project_grant_batch.sql", project, keys)
                .fetch_all(&mut *transaction)
                .await?
                .into_iter()
                .map(StoredAuthorityRecord::Grant)
                .collect()
        }
        RecordKind::StandDownAcknowledgement => sqlx_turso::query_file_as!(
            StoredStandDownAck,
            "src/project_stand_down_ack_batch.sql",
            project,
            keys
        )
        .fetch_all(&mut *transaction)
        .await?
        .into_iter()
        .map(StoredAuthorityRecord::StandDownAck)
        .collect(),
        _ => return Err(ProjectStorageError::InvalidFeed),
    })
}
