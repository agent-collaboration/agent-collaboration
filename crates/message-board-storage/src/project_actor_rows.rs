use crate::{ImportRowKind, ProjectStorageError, ProjectWriteTransaction};
use message_board::{RecordId, RecordedActor};
use sqlx::Executor;
use sqlx_turso::Turso;

pub(crate) fn decode_actor(
    actor_id: &str,
    canonical_identity: &str,
) -> Result<RecordedActor, ProjectStorageError> {
    RecordId::try_from(actor_id.to_owned())
        .map_err(|_| ProjectStorageError::InvalidActor { field: "actorId" })?;
    let actor = serde_json::from_str::<RecordedActor>(canonical_identity).map_err(|_| {
        ProjectStorageError::InvalidActor {
            field: "canonicalIdentity",
        }
    })?;
    if serde_json::to_string(&actor)? != canonical_identity {
        return Err(ProjectStorageError::InvalidActor {
            field: "canonicalIdentity",
        });
    }
    Ok(actor)
}

pub(crate) async fn actor_id<TScope>(
    scope: &mut TScope,
    actor: &RecordedActor,
) -> Result<Option<RecordId>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let canonical = serde_json::to_string(actor)?;
    let row = sqlx_turso::query!(
        r#"SELECT actor_id AS "actor_id!: String", canonical_identity AS "canonical_identity!: String" FROM actors WHERE canonical_identity = ?"#,
        canonical
    ).fetch_optional(&mut *scope).await?;
    row.map(|row| {
        if decode_actor(&row.actor_id, &row.canonical_identity)? != *actor {
            return Err(ProjectStorageError::InvalidActor {
                field: "canonicalIdentity",
            });
        }
        RecordId::try_from(row.actor_id)
            .map_err(|_| ProjectStorageError::InvalidActor { field: "actorId" })
    })
    .transpose()
}

impl ProjectWriteTransaction {
    pub(crate) async fn intern_actor(
        &mut self,
        actor: &RecordedActor,
    ) -> Result<RecordId, ProjectStorageError> {
        if let Some(id) = actor_id(&mut self.transaction, actor).await? {
            return Ok(id);
        }
        let canonical = serde_json::to_string(actor)?;
        let id = self
            .admission
            .row_key(ImportRowKind::Actor, &[&canonical])?;
        let encoded_id = id.as_str();
        sqlx_turso::query!(
            "INSERT INTO actors(actor_id, canonical_identity) VALUES (?, ?)",
            encoded_id,
            canonical
        )
        .execute(&mut self.transaction)
        .await?;
        Ok(id)
    }
}
