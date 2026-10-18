//! Cooldown selects the exact maximum Router time from indexed metadata, then one body.
use crate::{ProjectStorageError, ProjectWriteTransaction};
use chrono::{DateTime, Utc};
use message_board::*;

struct PostTimeMetadata {
    id: String,
    router_time: String,
}
impl ProjectWriteTransaction {
    pub async fn latest_cooldown_post_key(
        &mut self,
        actor: &Identity,
    ) -> Result<Option<RecordRef>, ProjectStorageError> {
        let project = self.project_id.as_str();
        let rows = match actor {
            Identity::Human { human_id } => {
                let human = human_id.as_str();
                sqlx_turso::query_as!(PostTimeMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.envelope.routerTime')) AS "router_time!: String" FROM record_identities WHERE kind = 'post' AND json_extract(payload, '$.record.placement.kind') = 'project' AND json_extract(payload, '$.record.actor.kind') = 'human' AND json_extract(payload, '$.record.placement.projectId') = ? AND json_extract(payload, '$.record.actor.humanId') = ? ORDER BY id"#, project, human).fetch_all(&mut *self.transaction).await?
            }
            Identity::Session { session } => {
                let service = session.endpoint.service_id.as_str();
                let endpoint = session.endpoint.endpoint_id.as_str();
                let session = session.session_id.as_str();
                sqlx_turso::query_as!(PostTimeMetadata, r#"SELECT id AS "id!: String", json_quote(json_extract(payload, '$.record.envelope.routerTime')) AS "router_time!: String" FROM record_identities WHERE kind = 'post' AND json_extract(payload, '$.record.placement.kind') = 'project' AND json_extract(payload, '$.record.actor.kind') = 'session' AND json_extract(payload, '$.record.placement.projectId') = ? AND json_extract(payload, '$.record.actor.session.endpoint.serviceId') = ? AND json_extract(payload, '$.record.actor.session.endpoint.endpointId') = ? AND json_extract(payload, '$.record.actor.session.sessionId') = ? ORDER BY id"#, project, service, endpoint, session).fetch_all(&mut *self.transaction).await?
            }
        };
        // DateTime comparison preserves sub-millisecond precision and regressed clocks.
        // Lexicographic RFC3339 sorting does not order omitted fractional seconds correctly.
        let mut latest: Option<(DateTime<Utc>, RecordRef)> = None;
        for row in rows {
            let reference = RecordRef {
                kind: RecordKind::Post,
                id: RecordId::try_from(row.id).map_err(|_| ProjectStorageError::InvalidFeed)?,
            };
            let time: DateTime<Utc> = serde_json::from_str(&row.router_time).map_err(|_| {
                ProjectStorageError::InvalidRecord {
                    record: reference.clone(),
                    field: "payload", // Keep the existing attributed invalid-post diagnostic.
                }
            })?;
            if latest.as_ref().is_none_or(|(previous, _)| time > *previous) {
                latest = Some((time, reference));
            }
        }
        Ok(latest.map(|(_, reference)| reference))
    }
}
