//! Current plan, guard results and prior matching move operands use metadata selections.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

fn reference(kind: RecordKind, id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
async fn current_plan_key(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Option<RecordRef>, ProjectStorageError> {
    sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'planRevision' AND json_extract(payload, '$.record.supersededBy') IS NULL ORDER BY json_extract(payload, '$.record.revision') DESC, id DESC LIMIT 1"#).fetch_optional(connection).await?.map(|id| reference(RecordKind::PlanRevision, id)).transpose()
}
struct GuardMetadataRow {
    id: String,
    guard: String,
    actor: String,
    position: String,
}
async fn guard_fact_keys(
    connection: &mut sqlx_turso::TursoConnection,
    project: &ProjectId,
    guards: &[GuardRequirement],
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    if guards.is_empty() {
        return Ok(Vec::new());
    }
    let project = project.as_str();
    let rows = sqlx_turso::query_as!(GuardMetadataRow, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.kind.fact.guard')) AS "guard!: String", json_quote(json_extract(payload, '$.record.kind.fact.evaluator.actor.actor')) AS "actor!: String", json_quote(json_extract(payload, '$.record.envelope.position')) AS "position!: String" FROM record_identities WHERE kind = 'fact' AND json_extract(payload, '$.record.kind.kind') = 'evaluatorResult' AND json_extract(payload, '$.record.kind.fact.subject.kind') = 'project' AND json_extract(payload, '$.record.kind.fact.subject.id') = ?"#, project).fetch_all(connection).await?;
    // The existing rule interprets guard strings through ResourceName; it does not
    // compare their JSON encoding. Select only the latest required guard/evaluator bodies.
    let mut selected: std::collections::BTreeMap<usize, (FeedPosition, RecordRef)> =
        std::collections::BTreeMap::new();
    let mut invalid_evaluator = None;
    for row in rows {
        let key = reference(RecordKind::Fact, row.id)?;
        let invalid = |field| ProjectStorageError::InvalidRecord {
            record: key.clone(),
            field,
        };
        let text: String = serde_json::from_str(&row.guard).map_err(|_| invalid("guard"))?;
        let name = ResourceName::try_from(text).map_err(|_| invalid("guard"))?;
        if !guards.iter().any(|guard| guard.name == name) {
            continue;
        }
        let actor: ActorKind =
            serde_json::from_str(&row.actor).map_err(|_| invalid("evaluator"))?;
        let position: RecordPosition =
            serde_json::from_str(&row.position).map_err(|_| invalid("position"))?;
        let RecordPosition::Project(cursor) = position else {
            return Err(invalid("position"));
        };
        match actor {
            ActorKind::Declared(identity) => {
                for (index, _) in guards
                    .iter()
                    .enumerate()
                    .filter(|(_, guard)| guard.name == name && guard.evaluator == identity)
                {
                    let newer = selected.get(&index).is_none_or(|(position, previous)| {
                        (cursor.position, &key.id) > (*position, &previous.id)
                    });
                    if newer {
                        selected.insert(index, (cursor.position, key.clone()));
                    }
                }
            }
            ActorKind::Router { .. } => {
                invalid_evaluator.get_or_insert(key);
            }
        }
    }
    let mut keys: Vec<_> = selected.into_values().map(|(_, key)| key).collect();
    keys.extend(invalid_evaluator);
    Ok(keys)
}
async fn prior_rejected_move_keys(
    connection: &mut sqlx_turso::TursoConnection,
    name: &MoveName,
    actor: &Identity,
) -> Result<Vec<RecordRef>, ProjectStorageError> {
    let name = name.as_str();
    let rows = match actor {
        Identity::Human { human_id } => {
            let human = human_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'move' AND json_extract(payload, '$.record.moveName') = ? AND json_extract(payload, '$.record.requester.actor.identity.kind') = 'human' AND json_extract(payload, '$.record.requester.actor.identity.humanId') = ? AND json_extract(payload, '$.record.outcome.kind') = 'rejected' ORDER BY id"#, name, human).fetch_all(connection).await?
        }
        Identity::Session { session } => {
            let service = session.endpoint.service_id.as_str();
            let endpoint = session.endpoint.endpoint_id.as_str();
            let session = session.session_id.as_str();
            sqlx_turso::query_scalar!(r#"SELECT id AS "id!: String" FROM record_identities WHERE kind = 'move' AND json_extract(payload, '$.record.moveName') = ? AND json_extract(payload, '$.record.requester.actor.identity.kind') = 'session' AND json_extract(payload, '$.record.requester.actor.identity.session.endpoint.serviceId') = ? AND json_extract(payload, '$.record.requester.actor.identity.session.endpoint.endpointId') = ? AND json_extract(payload, '$.record.requester.actor.identity.session.sessionId') = ? AND json_extract(payload, '$.record.outcome.kind') = 'rejected' ORDER BY id"#, name, service, endpoint, session).fetch_all(connection).await?
        }
    };
    rows.into_iter()
        .map(|id| reference(RecordKind::Move, id))
        .collect()
}
impl ProjectWriteTransaction {
    pub async fn current_plan_key(&mut self) -> Result<Option<RecordRef>, ProjectStorageError> {
        current_plan_key(&mut self.transaction).await
    }
    pub async fn guard_fact_keys(
        &mut self,
        guards: &[GuardRequirement],
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        guard_fact_keys(&mut self.transaction, &self.project_id, guards).await
    }
    pub async fn prior_rejected_move_keys(
        &mut self,
        name: &MoveName,
        actor: &Identity,
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        prior_rejected_move_keys(&mut self.transaction, name, actor).await
    }
}
impl ProjectSnapshot {
    pub async fn current_plan_key(&mut self) -> Result<Option<RecordRef>, ProjectStorageError> {
        current_plan_key(&mut self.transaction).await
    }
    pub async fn guard_fact_keys(
        &mut self,
        guards: &[GuardRequirement],
    ) -> Result<Vec<RecordRef>, ProjectStorageError> {
        guard_fact_keys(&mut self.transaction, &self.project_id, guards).await
    }
}
