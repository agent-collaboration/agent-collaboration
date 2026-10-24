//! Authority and stand-down operands use typed subject and exact actor keys.
use crate::project_actor_rows::actor_id;
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;
use sqlx::Executor;
use sqlx_turso::Turso;

fn role_reference(id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind: RecordKind::RoleAssignment,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
struct ActorRoleMetadata {
    id: String,
    ended: Option<String>,
}
enum ActorRoleSelection {
    Active,
    StoodDown,
}
fn select_actor_role_keys(
    rows: Vec<ActorRoleMetadata>,
    selection: ActorRoleSelection,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let mut keys = Vec::new();
    for row in rows {
        let key = role_reference(row.id)?;
        let ended: Option<RoleEnd> = row
            .ended
            .map(|value| crate::project_record_codec::stored_json(&value, &key, "ended"))
            .transpose()?;
        if matches!(
            (&selection, ended),
            (ActorRoleSelection::Active, None)
                | (
                    ActorRoleSelection::StoodDown,
                    Some(RoleEnd::StoodDown { .. })
                )
        ) {
            keys.push(key);
        }
    }
    Ok(keys)
}

async fn active_role_keys<TScope>(
    connection: &mut TScope,
    subject: &RoleSubject,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let rows = match subject {
        RoleSubject::Project(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE project_subject=? ORDER BY assignment_id"#, id).fetch_all(&mut *connection).await?
        }
        RoleSubject::Workstream(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE workstream_subject=? ORDER BY assignment_id"#, id).fetch_all(&mut *connection).await?
        }
    };
    select_actor_role_keys(rows, ActorRoleSelection::Active)
}
async fn active_role_selection_keys<TScope>(
    connection: &mut TScope,
    subject: &RoleSubject,
    role: AuthorityRole,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let role = role.as_str();
    let rows = match subject {
        RoleSubject::Project(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE project_subject=? AND role=? ORDER BY assignment_id"#, id, role).fetch_all(&mut *connection).await?
        }
        RoleSubject::Workstream(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE workstream_subject=? AND role=? ORDER BY assignment_id"#, id, role).fetch_all(&mut *connection).await?
        }
    };
    select_actor_role_keys(rows, ActorRoleSelection::Active)
}
async fn latest_project_lead_key<TScope>(
    connection: &mut TScope,
    project: &ProjectId,
) -> Result<Option<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let project = project.as_str();
    sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "id!: String" FROM role_assignments WHERE project_subject=? AND role='lead' ORDER BY position DESC, assignment_id DESC LIMIT 1"#, project).fetch_optional(&mut *connection).await?.map(role_reference).transpose()
}
async fn actor_role_keys<TScope>(
    connection: &mut TScope,
    actor: &Identity,
    selection: ActorRoleSelection,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(actor) = actor_id(connection, &RecordedActor::declared(actor.clone())).await? else {
        return Ok(Vec::new());
    };
    let actor = actor.as_str();
    let rows = sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE holder=? ORDER BY assignment_id"#, actor).fetch_all(&mut *connection).await?;
    select_actor_role_keys(rows, selection)
}
async fn stand_down_ack_keys<TScope>(
    connection: &mut TScope,
    actor: &Identity,
    assignments: &[RoleAssignmentId],
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    if assignments.is_empty() {
        return Ok(Vec::new());
    }
    let Some(actor) = actor_id(connection, &RecordedActor::declared(actor.clone())).await? else {
        return Ok(Vec::new());
    };
    let actor = actor.as_str();
    let keys = serde_json::to_string(assignments)?;
    let rows = sqlx_turso::query_scalar!(r#"SELECT (SELECT a.acknowledgement_id FROM stand_down_acknowledgements a WHERE a.assignment_id=CAST(k.value AS TEXT) AND a.actor_id=? ORDER BY a.through_position DESC, a.acknowledgement_id DESC LIMIT 1) AS "id?: String" FROM json_each(?) k"#, actor, keys).fetch_all(&mut *connection).await?;
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
async fn owner_grant_use_key<TScope>(
    connection: &mut TScope,
    grant: &OwnerGrantId,
) -> Result<Option<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let grant = grant.as_str();
    sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "id!: String" FROM role_assignments WHERE owner_grant=? LIMIT 1"#, grant).fetch_optional(&mut *connection).await?.map(role_reference).transpose()
}
pub(crate) async fn active_actor_subject_role_keys<TScope>(
    connection: &mut TScope,
    actor: &Identity,
    subject: &RoleSubject,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(actor) = actor_id(connection, &RecordedActor::declared(actor.clone())).await? else {
        return Ok(Vec::new());
    };
    let actor = actor.as_str();
    let rows = match subject {
        RoleSubject::Project(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE holder=? AND project_subject=? ORDER BY assignment_id"#, actor, id).fetch_all(&mut *connection).await?
        }
        RoleSubject::Workstream(id) => {
            let id = id.as_str();
            sqlx_turso::query_as!(ActorRoleMetadata, r#"SELECT assignment_id AS "id!: String", ended AS "ended?: String" FROM role_assignments WHERE holder=? AND workstream_subject=? ORDER BY assignment_id"#, actor, id).fetch_all(&mut *connection).await?
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
        assignments: &[RoleAssignmentId],
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

// The stand-down metadata gate validates closed end states before rule-key selection.
enum ActorRolePurpose {
    ImplicitTrail,
    ProjectRoleWitness,
}
async fn active_actor_rule_keys<TScope>(
    connection: &mut TScope,
    actor: &Identity,
    purpose: ActorRolePurpose,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let Some(actor) = actor_id(connection, &RecordedActor::declared(actor.clone())).await? else {
        return Ok(Vec::new());
    };
    let actor = actor.as_str();
    let any_role = matches!(purpose, ActorRolePurpose::ProjectRoleWitness);
    let limit = if any_role { 1_i64 } else { i64::MAX };
    sqlx_turso::query_scalar!(r#"SELECT assignment_id AS "id!: String" FROM role_assignments WHERE holder=? AND ended IS NULL AND (? OR role IN ('lead','implementer','verifier')) ORDER BY assignment_id LIMIT ?"#, actor, any_role, limit).fetch_all(&mut *connection).await?.into_iter().map(role_reference).collect()
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
