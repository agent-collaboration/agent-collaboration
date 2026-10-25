use crate::project_record_codec::{ProjectRowEnvelope, require_one_row};
use crate::{ImportRowKind, ProjectStorageError, ProjectWriteTransaction};
use message_board::*;

impl ProjectWriteTransaction {
    pub(crate) async fn write_trail_row(
        &mut self,
        row: &WorkTrailRow,
        envelope: &ProjectRowEnvelope,
    ) -> Result<(), ProjectStorageError> {
        let id = row.row_id.as_str();
        let actor = envelope.actor_id.as_str();
        let request = envelope.request_id.as_deref();
        let subject = serde_json::to_string(&row.subject)?;
        let text = row.text.as_str();
        let changed = sqlx_turso::query!(
            "INSERT INTO work_trail_rows(row_id, actor_id, request_id, epoch, position, router_time, time_basis, subject, text, redacted) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            id, actor, request, envelope.epoch, envelope.position, envelope.router_time,
            envelope.time_basis, subject, text, row.redacted
        ).execute(&mut self.transaction).await?;
        require_one_row(changed.rows_affected(), row.row_id.clone().into())?;
        for (place, tag) in row.tags.iter().enumerate() {
            let place = i64::try_from(place).map_err(|_| ProjectStorageError::InvalidFeed)?;
            let ordinal = place.to_string();
            let key = self
                .admission
                .row_key(ImportRowKind::WorkTrailTag, &[id, &ordinal])?;
            let key = key.as_str();
            let tag = tag.as_str();
            sqlx_turso::query!(
                "INSERT INTO work_trail_tags(tag_id, row_id, place, tag) VALUES (?, ?, ?, ?)",
                key,
                id,
                place,
                tag
            )
            .execute(&mut self.transaction)
            .await?;
        }
        for (place, target) in row.links.iter().enumerate() {
            let place = i64::try_from(place).map_err(|_| ProjectStorageError::InvalidFeed)?;
            let ordinal = place.to_string();
            let key = self
                .admission
                .row_key(ImportRowKind::WorkTrailLink, &[id, &ordinal])?;
            let key = key.as_str();
            let target = serde_json::to_string(target)?;
            sqlx_turso::query!(
                "INSERT INTO work_trail_links(link_id, row_id, place, target) VALUES (?, ?, ?, ?)",
                key,
                id,
                place,
                target
            )
            .execute(&mut self.transaction)
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn write_trail_redaction(
        &mut self,
        row: &TrailRedaction,
    ) -> Result<(), ProjectStorageError> {
        let id = row.row.as_str();
        let changed = sqlx_turso::query!(
            "UPDATE work_trail_rows SET redacted=? WHERE row_id=?",
            row.redacted,
            id
        )
        .execute(&mut self.transaction)
        .await?;
        require_one_row(changed.rows_affected(), row.row.clone().into())
    }
}
