//! Authority and stand-down operands come from subject/actor metadata indexes.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

fn role_reference(id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind: RecordKind::RoleAssignment,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
async fn active_role_keys(
    connection: &mut sqlx_turso::TursoConnection,
    subject: &RoleSubject,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    // Retained assignment history is excluded by the existing active-holder indexes.
    let rows = match subject {
        RoleSubject::Project(project) => {
            let id = project.as_str();
            sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "assignment_id!: String" FROM role_assignments INDEXED BY active_project_roles WHERE project_subject = ? AND active = 1 ORDER BY assignment_id"#, id).fetch_all(connection).await?
        }
        RoleSubject::Workstream(workstream) => {
            let id = workstream.as_str();
            sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "assignment_id!: String" FROM role_assignments INDEXED BY active_workstream_roles WHERE workstream_subject = ? AND active = 1 ORDER BY assignment_id"#, id).fetch_all(connection).await?
        }
    };
    rows.into_iter().map(role_reference).collect()
}
async fn latest_project_lead_key(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
) -> Result<Option<RecordRef>, ProjectStorageError> {
    let project = project.as_str();
    let id = sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.subject.kind') = 'project' AND json_extract(payload, '$.record.subject.subject') = ? AND json_extract(payload, '$.record.role') = 'lead' ORDER BY json_extract(payload, '$.record.envelope.position.position.position') DESC, id DESC LIMIT 1"#, project).fetch_optional(connection).await?;
    id.map(role_reference).transpose()
}
async fn active_role_selection_keys(
    connection: &mut sqlx_turso::TursoConnection,
    subject: &RoleSubject,
    role: AuthorityRole,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let role = role.as_str();
    let ids = match subject {
        RoleSubject::Project(id) => {
            let id = id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "assignment_id!: String" FROM role_assignments WHERE project_subject = ? AND role = ? AND active = 1 ORDER BY assignment_id"#, id, role).fetch_all(connection).await?
        }
        RoleSubject::Workstream(id) => {
            let id = id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "assignment_id!: String" FROM role_assignments WHERE workstream_subject = ? AND role = ? AND active = 1 ORDER BY assignment_id"#, id, role).fetch_all(connection).await?
        }
    };
    ids.into_iter().map(role_reference).collect()
}
enum ActorRoleSelection {
    Active,
    StoodDown,
}
struct ActorRoleMetadata {
    id: String,
    ended: String,
}
fn select_actor_role_keys(
    rows: Vec<ActorRoleMetadata>,
    selection: ActorRoleSelection,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let mut keys = Vec::new();
    for row in rows {
        let key = role_reference(row.id)?;
        let ended: Option<RoleEnd> =
            serde_json::from_str(&row.ended).map_err(|_| ProjectStorageError::InvalidRecord {
                record: key.clone(),
                field: "ended",
            })?;
        let selected = matches!(
            (&selection, ended),
            (ActorRoleSelection::Active, None)
                | (
                    ActorRoleSelection::StoodDown,
                    Some(RoleEnd::StoodDown { .. })
                )
        );
        if selected {
            keys.push(key);
        }
    }
    Ok(keys)
}
async fn actor_role_keys(
    connection: &mut sqlx_turso::TursoConnection,
    actor: &Identity,
    selection: ActorRoleSelection,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    // End state is a rule input. Decode its closed enum before selecting bodies;
    // filtering only known tags in SQL would make an unknown state disappear.
    let rows = match actor {
        Identity::Human { human_id } => {
            let human = human_id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.ended')) AS "ended!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'human' AND json_extract(payload, '$.record.holder.actor.identity.humanId') = ? ORDER BY id"#, human).fetch_all(connection).await?
        }
        Identity::Session { session } => {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.ended')) AS "ended!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'session' AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.serviceId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.endpointId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.sessionId') = ? ORDER BY id"#, service, endpoint, session).fetch_all(connection).await?
        }
    };
    select_actor_role_keys(rows, selection)
}
async fn stand_down_ack_keys(
    connection: &mut sqlx_turso::TursoConnection,
    actor: &Identity,
    assignments: &[(RoleAssignmentId, EpochNumber)],
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    let keys = serde_json::to_string(assignments)?;
    let rows = match actor {
        Identity::Human { human_id } => {
            let human = human_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT (SELECT a.acknowledgement_id FROM stand_down_acknowledgements a JOIN record_identities r ON r.kind = 'standDownAcknowledgement' AND r.id = a.acknowledgement_id WHERE a.assignment_id = CAST(json_extract(k.value, '$[0]') AS TEXT) AND a.through_epoch = CAST(json_extract(k.value, '$[1]') AS INTEGER) AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.kind') = 'human' AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.humanId') = ? ORDER BY a.through_position DESC, a.acknowledgement_id DESC LIMIT 1) AS "id: String" FROM json_each(?) k"#, human, keys).fetch_all(connection).await?
        }
        Identity::Session { session } => {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT (SELECT a.acknowledgement_id FROM stand_down_acknowledgements a JOIN record_identities r ON r.kind = 'standDownAcknowledgement' AND r.id = a.acknowledgement_id WHERE a.assignment_id = CAST(json_extract(k.value, '$[0]') AS TEXT) AND a.through_epoch = CAST(json_extract(k.value, '$[1]') AS INTEGER) AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.kind') = 'session' AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.session.endpoint.serviceId') = ? AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.session.endpoint.endpointId') = ? AND json_extract(r.payload, '$.record.envelope.actor.actor.identity.session.sessionId') = ? ORDER BY a.through_position DESC, a.acknowledgement_id DESC LIMIT 1) AS "id: String" FROM json_each(?) k"#, service, endpoint, session, keys).fetch_all(connection).await?
        }
    };
    rows.into_iter()
        .flatten()
        .map(|id| {
            Ok(RecordRef {
                kind: RecordKind::StandDownAcknowledgement,
                id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
            })
        })
        .collect()
}
async fn owner_grant_use_key(
    connection: &mut sqlx_turso::TursoConnection,
    grant: &OwnerGrantId,
) -> Result<Option<RecordRef>, ProjectStorageError> {
    let grant = grant.as_str();
    sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "assignment_id!: String" FROM role_assignments WHERE owner_grant = ? LIMIT 1"#, grant).fetch_optional(connection).await?.map(role_reference).transpose()
}
pub(crate) async fn active_actor_subject_role_keys(
    connection: &mut sqlx_turso::TursoConnection,
    actor: &Identity,
    subject: &RoleSubject,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let (subject_kind, subject_id) = match subject {
        RoleSubject::Project(id) => ("project", id.as_str()),
        RoleSubject::Workstream(id) => ("workstream", id.as_str()),
    };
    let rows = match actor {
        Identity::Human { human_id } => {
            let human = human_id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.ended')) AS "ended!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'human' AND json_extract(payload, '$.record.holder.actor.identity.humanId') = ? AND json_extract(payload, '$.record.subject.kind') = ? AND json_extract(payload, '$.record.subject.subject') = ? ORDER BY id"#, human, subject_kind, subject_id).fetch_all(connection).await?
        }
        Identity::Session { session } => {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.ended')) AS "ended!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'session' AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.serviceId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.endpointId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.sessionId') = ? AND json_extract(payload, '$.record.subject.kind') = ? AND json_extract(payload, '$.record.subject.subject') = ? ORDER BY id"#, service, endpoint, session, subject_kind, subject_id).fetch_all(connection).await?
        }
    };
    select_actor_role_keys(rows, ActorRoleSelection::Active)
}
impl ProjectWriteTransaction {
    pub async fn active_roles(
        &mut self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Result<Vec<RoleAssignment>, ProjectStorageError> {
        let keys = active_role_selection_keys(&mut self.transaction, subject, role).await?;
        let mut assignments = Vec::new();
        for stored in self.record_batch(&keys).await? {
            let reference = stored.reference().clone();
            match stored.decode()? {
                Record::RoleAssignment(assignment)
                    if assignment.subject() == *subject
                        && assignment.role() == role
                        && assignment.ended.is_none() =>
                {
                    assignments.push(assignment)
                }
                _ => {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: reference,
                        field: "roleAssignment",
                    });
                }
            }
        }
        if assignments.len() != keys.len()
            || (role.cardinality() == RoleCardinality::One && assignments.len() > 1)
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(assignments)
    }
    pub async fn active_actor_subject_role_keys(
        &mut self,
        actor: &Identity,
        subject: &RoleSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_actor_subject_role_keys(&mut self.transaction, actor, subject).await
    }
    pub async fn active_role_selection_keys(
        &mut self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_role_selection_keys(&mut self.transaction, subject, role).await
    }
    pub async fn active_role_keys(
        &mut self,
        subject: &RoleSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_role_keys(&mut self.transaction, subject).await
    }
    pub async fn latest_project_lead_key(
        &mut self,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        latest_project_lead_key(&mut self.transaction, &self.project_id).await
    }
    pub async fn active_actor_role_keys(
        &mut self,
        actor: &Identity,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        actor_role_keys(&mut self.transaction, actor, ActorRoleSelection::Active).await
    }
    pub async fn stood_down_actor_role_keys(
        &mut self,
        actor: &Identity,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        actor_role_keys(&mut self.transaction, actor, ActorRoleSelection::StoodDown).await
    }
    pub async fn stand_down_ack_keys(
        &mut self,
        actor: &Identity,
        assignments: &[(RoleAssignmentId, EpochNumber)],
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        stand_down_ack_keys(&mut self.transaction, actor, assignments).await
    }
    pub async fn owner_grant_use_key(
        &mut self,
        grant: &OwnerGrantId,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        owner_grant_use_key(&mut self.transaction, grant).await
    }
}
impl ProjectSnapshot {
    pub async fn active_role_selection_keys(
        &mut self,
        subject: &RoleSubject,
        role: AuthorityRole,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_role_selection_keys(&mut self.transaction, subject, role).await
    }
    pub async fn active_role_keys(
        &mut self,
        subject: &RoleSubject,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_role_keys(&mut self.transaction, subject).await
    }
    pub async fn active_actor_role_keys(
        &mut self,
        actor: &Identity,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        actor_role_keys(&mut self.transaction, actor, ActorRoleSelection::Active).await
    }
}

// The rule chooses its role keys; bodies are validated by the admission operand batch.
// The existing stand-down metadata gate still validates this actor's closed end states.
enum ActorRolePurpose {
    ImplicitTrail,
    ProjectRoleWitness,
}
async fn active_actor_rule_keys(
    connection: &mut sqlx_turso::TursoConnection,
    actor: &Identity,
    purpose: ActorRolePurpose,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let any_role = matches!(purpose, ActorRolePurpose::ProjectRoleWitness);
    let limit = if any_role { 1_i64 } else { i64::MAX };
    let ids = match actor {
        Identity::Human { human_id } => {
            let human = human_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'human' AND json_extract(payload, '$.record.holder.actor.identity.humanId') = ? AND json_extract(payload, '$.record.ended') IS NULL AND (? OR json_extract(payload, '$.record.role') IN ('lead','implementer','verifier')) ORDER BY id LIMIT ?"#, human, any_role, limit).fetch_all(connection).await?
        }
        Identity::Session { session } => {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'roleAssignment' AND json_extract(payload, '$.record.holder.actor.identity.kind') = 'session' AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.serviceId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.endpoint.endpointId') = ? AND json_extract(payload, '$.record.holder.actor.identity.session.sessionId') = ? AND json_extract(payload, '$.record.ended') IS NULL AND (? OR json_extract(payload, '$.record.role') IN ('lead','implementer','verifier')) ORDER BY id LIMIT ?"#, service, endpoint, session, any_role, limit).fetch_all(connection).await?
        }
    };
    ids.into_iter().map(role_reference).collect()
}
impl ProjectWriteTransaction {
    pub async fn active_trail_assignment_keys(
        &mut self,
        actor: &Identity,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        active_actor_rule_keys(
            &mut self.transaction,
            actor,
            ActorRolePurpose::ImplicitTrail,
        )
        .await
    }
    pub async fn project_role_witness_key(
        &mut self,
        actor: &Identity,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        Ok(active_actor_rule_keys(
            &mut self.transaction,
            actor,
            ActorRolePurpose::ProjectRoleWitness,
        )
        .await?
        .into_iter()
        .next())
    }
}
