//! Bounded intent operands are selected by their cause and stable actor identity.
use crate::{ProjectSnapshot, ProjectStorageError, ProjectWriteTransaction};
use message_board::{
    EpochNumber, FeedCursor, FeedPosition, NotifyIntent, NotifyIntentId, Record, RecordKind,
    RecordRef, ServiceId,
};

pub struct IntentPageKey {
    pub intent: NotifyIntentId,
    pub cursor: FeedCursor,
}

impl ProjectSnapshot {
    pub async fn validate_intent_scan_range(
        &mut self,
        after: FeedPosition,
        through: FeedPosition,
        installation: &ServiceId,
    ) -> Result<(), ProjectStorageError> {
        if through < after || through > self.high_water.position {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let expected = through.get() - after.get();
        let after = i64::try_from(after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let through = i64::try_from(through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let installation = installation.as_str();
        let row = sqlx_turso::query!(r#"SELECT COUNT(*) AS "count!: i64", COALESCE(SUM(CASE WHEN e.holder = ? AND e.start_position >= 0 AND f.position > e.start_position AND f.position <= COALESCE((SELECT next.start_position FROM writer_epochs next WHERE next.epoch > e.epoch ORDER BY next.epoch LIMIT 1), ?) THEN 0 ELSE 1 END), 0) AS "invalid!: i64" FROM project_feed f LEFT JOIN writer_epochs e ON e.epoch = f.epoch WHERE f.position > ? AND f.position <= ?"#, installation, through, after, through).fetch_one(&mut *self.transaction).await?;
        if u64::try_from(row.count).ok() != Some(expected) || row.invalid != 0 {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(())
    }
    pub async fn intent_page_keys(
        &mut self,
        after: FeedPosition,
        through: FeedPosition,
        limit: u64,
    ) -> Result<Vec<IntentPageKey>, ProjectStorageError> {
        let after = i64::try_from(after.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let through = i64::try_from(through.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let limit = i64::try_from(limit).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let rows = sqlx_turso::query!(r#"SELECT position AS "position!: i64", epoch AS "epoch!: i64", record_id AS "record_id!: String" FROM project_feed INDEXED BY feed_by_kind_position WHERE record_kind = 'notifyIntent' AND position > ? AND position <= ? ORDER BY position LIMIT ?"#, after, through, limit)
            .fetch_all(&mut *self.transaction).await?;
        rows.into_iter()
            .map(|row| {
                Ok(IntentPageKey {
                    intent: NotifyIntentId::try_from(row.record_id)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    cursor: FeedCursor {
                        epoch: EpochNumber::try_from(
                            u64::try_from(row.epoch)
                                .map_err(|_| ProjectStorageError::InvalidFeed)?,
                        )
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                        position: FeedPosition::try_from(
                            u64::try_from(row.position)
                                .map_err(|_| ProjectStorageError::InvalidFeed)?,
                        )
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                    },
                })
            })
            .collect()
    }
}

impl ProjectWriteTransaction {
    pub async fn cause_intents(
        &mut self,
        cause: &RecordRef,
    ) -> Result<Vec<NotifyIntent>, ProjectStorageError> {
        let kind = crate::project_record_io::record_kind_tag(cause.kind);
        let cause_id = cause.id.as_str();
        let ids = sqlx_turso::query_scalar!(r#"SELECT intent_id AS "intent_id!: String" FROM notify_intents WHERE cause_kind = ? AND cause_id = ? ORDER BY intent_id"#, kind, cause_id).fetch_all(&mut *self.transaction).await?;
        let keys = ids
            .into_iter()
            .map(|id| {
                Ok(RecordRef {
                    kind: RecordKind::NotifyIntent,
                    id: message_board::RecordId::try_from(id)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                })
            })
            .collect::<Result<Vec<_>, ProjectStorageError>>()?;
        let mut intents = Vec::new();
        for record in self.record_batch(&keys).await? {
            let reference = record.reference().clone();
            match record.decode()? {
                Record::NotifyIntent(intent) if intent.cause == *cause => intents.push(intent),
                _ => {
                    return Err(ProjectStorageError::InvalidRecord {
                        record: reference,
                        field: "cause",
                    });
                }
            }
        }
        if intents.len() != keys.len() {
            return Err(ProjectStorageError::InvalidFeed);
        }
        Ok(intents)
    }
}
