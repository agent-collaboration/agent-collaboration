//! Cooldown uses known Router time only and then reads the selected immutable post.
use crate::{ProjectStorageError, ProjectWriteTransaction};
use message_board::{Identity, RecordId, RecordKind, RecordRef, RecordTime, RecordedActor};

impl ProjectWriteTransaction {
    pub async fn latest_cooldown_post_key(
        &mut self,
        actor: &Identity,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        let Some(actor) = crate::project_actor_rows::actor_id(
            &mut self.transaction,
            &RecordedActor::declared(actor.clone()),
        )
        .await?
        else {
            return Ok(None);
        };
        let actor = actor.as_str();
        // Stored times use fixed nanosecond UTC formatting. Decode before comparison so a
        // corrupt timestamp is refused rather than winning or disappearing in SQL ordering.
        let rows = sqlx_turso::query!(r#"SELECT post_id AS "id!: String", router_time AS "router_time?: String", time_basis AS "time_basis!: String" FROM posts WHERE actor_id=? AND placement_kind='project' ORDER BY post_id"#, actor).fetch_all(&mut self.transaction).await?;
        let mut latest = None;
        for row in rows {
            let reference = RecordRef {
                kind: RecordKind::Post,
                id: RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?,
            };
            if let RecordTime::Router(time) = crate::project_record_codec::decode_record_time(
                row.router_time,
                &row.time_basis,
                &reference,
            )? && latest.as_ref().is_none_or(|(previous, _)| time > *previous)
            {
                latest = Some((time, reference));
            }
        }
        Ok(latest.map(|(_, reference)| reference))
    }
}
