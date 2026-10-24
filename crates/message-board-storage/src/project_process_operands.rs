//! Current plan, guard results and prior matching move operands use metadata selections.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

fn reference(kind: RecordKind, id: String) -> Result<RecordRef, ProjectStorageError> {
    Ok(RecordRef {
        kind,
        id: RecordId::try_from(id).map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}
async fn current_plan_key<TScope>(
    connection: &mut TScope,
) -> Result<Option<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    sqlx_turso::query_scalar!(r#"SELECT revision_id AS "id!: String" FROM plan_revisions WHERE superseded_by IS NULL ORDER BY revision DESC, revision_id DESC LIMIT 1"#).fetch_optional(connection).await?.map(|id| reference(RecordKind::PlanRevision, id)).transpose()
}
struct GuardMetadataRow {
    id: String,
    guard: String,
    actor: String,
    epoch: i64,
    position: i64,
}
async fn guard_fact_keys<TScope>(
    connection: &mut TScope,
    project: &ProjectId,
    guards: &[GuardRequirement],
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    if guards.is_empty() {
        return Ok(Vec::new());
    }
    let project = project.as_str();
    let rows = sqlx_turso::query_as!(GuardMetadataRow, r#"SELECT fact_id AS "id!: String", json_quote(json_extract(fact, '$.fact.guard')) AS "guard!: String", json_extract(fact, '$.fact.evaluator.actor') AS "actor!: String", epoch AS "epoch!: i64", position AS "position!: i64" FROM facts WHERE json_extract(fact, '$.kind')='evaluatorResult' AND json_extract(fact, '$.fact.subject.kind')='project' AND json_extract(fact, '$.fact.subject.id')=?"#, project).fetch_all(&mut *connection).await?;
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
        let actor: RecordedActor =
            serde_json::from_str(&row.actor).map_err(|_| invalid("evaluator"))?;
        let cursor = FeedCursor {
            epoch: crate::project_record_codec::stored_counter(row.epoch, &key, "position")?,
            position: crate::project_record_codec::stored_counter(row.position, &key, "position")?,
        };
        match actor.declared_identity() {
            Some(identity) => {
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
            None => {
                invalid_evaluator.get_or_insert(key);
            }
        }
    }
    let mut keys: Vec<_> = selected.into_values().map(|(_, key)| key).collect();
    keys.extend(invalid_evaluator);
    Ok(keys)
}
async fn prior_rejected_move_keys<TScope>(
    connection: &mut TScope,
    name: &MoveName,
    actor: &Identity,
) -> Result<Vec<RecordRef>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: sqlx::Executor<'scope, Database = sqlx_turso::Turso>,
{
    let Some(actor) =
        crate::project_actor_rows::actor_id(connection, &RecordedActor::declared(actor.clone()))
            .await?
    else {
        return Ok(Vec::new());
    };
    let actor = actor.as_str();
    let name = name.as_str();
    sqlx_turso::query_scalar!(r#"SELECT move_id AS "id!: String" FROM moves WHERE move_name=? AND requester=? AND json_extract(outcome, '$.kind')='rejected' ORDER BY move_id"#, name, actor).fetch_all(&mut *connection).await?.into_iter().map(|id| reference(RecordKind::Move, id)).collect()
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
