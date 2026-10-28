//! An activation's last immutable membership fact survives same-ID reactivation.
use crate::{ProjectSnapshot, ProjectStorageError};
use message_board::{FeedContent, FeedCursor, Record, RecordId, SubscriptionMembership};

impl ProjectSnapshot {
    pub async fn membership_activation(
        &mut self,
        membership: &RecordId,
        activation: FeedCursor,
    ) -> Result<Option<SubscriptionMembership>, ProjectStorageError> {
        let epoch =
            i64::try_from(activation.epoch.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let position = i64::try_from(activation.position.get())
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row = sqlx_turso::query!(r#"SELECT f.position AS "position!: i64",f.epoch AS "epoch!: i64",f.record_id AS "record_id!: String",f.router_time AS "time?: String",f.time_basis AS "time_basis!: String" FROM project_feed f INDEXED BY feed_by_record WHERE f.record_kind='subscriptionMembership' AND f.record_id=? AND json_extract(f.content,'$.record.record.activatedAt.epoch')=? AND json_extract(f.content,'$.record.record.activatedAt.position')=? ORDER BY f.position DESC LIMIT 1"#, membership.as_str(), epoch, position)
            .fetch_optional(&mut self.transaction).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let key = self.feed_page_key(
            row.position,
            row.epoch,
            "subscriptionMembership".to_owned(),
            row.record_id,
            row.time,
            row.time_basis,
        )?;
        let entry = self
            .feed_entry_batch(std::slice::from_ref(&key))
            .await?
            .remove(&key.cursor.position)
            .ok_or(ProjectStorageError::InvalidFeed)?
            .decode(false)?;
        match entry.content {
            FeedContent::Shown(record) => match *record {
                Record::SubscriptionMembership(member)
                    if member.envelope.id == *membership && member.activated_at() == activation =>
                {
                    Ok(Some(member))
                }
                _ => Err(ProjectStorageError::InvalidFeed),
            },
            FeedContent::Redacted => Err(ProjectStorageError::InvalidFeed),
        }
    }
}
