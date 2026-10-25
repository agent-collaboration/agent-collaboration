//! Accepted-line boundaries are board data; writer authority remains in the library.

use crate::{
    ImportRowKind, ProjectSnapshot, ProjectStorageError, project_import_keys::RecordAdmission,
};
use fleet_sync_writer::WriteTransaction;
use message_board::{EpochLine, EpochLineId, EpochNumber, FeedCursor, FeedPosition, RecordId};
use sqlx::Executor;
use sqlx_turso::Turso;

struct StoredEpochLine {
    line_id: String,
    epoch: i64,
    inherited_through: i64,
}

impl StoredEpochLine {
    fn decode(self) -> Result<EpochLine, ProjectStorageError> {
        Ok(EpochLine {
            line_id: EpochLineId::try_from(self.line_id)
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
            epoch: EpochNumber::try_from(
                u64::try_from(self.epoch).map_err(|_| ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
            inherited_through: FeedPosition::try_from(
                u64::try_from(self.inherited_through)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
        })
    }
}

async fn latest_line<TScope>(scope: &mut TScope) -> Result<Option<EpochLine>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let row = sqlx_turso::query_as!(StoredEpochLine,
        r#"SELECT line_id AS "line_id!: String", epoch AS "epoch!: i64", inherited_through AS "inherited_through!: i64" FROM epoch_lines ORDER BY epoch DESC LIMIT 1"#
    ).fetch_optional(&mut *scope).await?;
    row.map(StoredEpochLine::decode).transpose()
}

pub(crate) async fn checkpoint<TScope>(
    scope: &mut TScope,
) -> Result<FeedCursor, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let rows = sqlx_turso::query!(r#"SELECT entry_id AS "entry_id!: String", epoch AS "epoch!: i64", position AS "position!: i64" FROM project_feed ORDER BY position DESC LIMIT 2"#)
        .fetch_all(&mut *scope).await?;
    let Some(row) = rows.first() else {
        return Ok(FeedCursor {
            epoch: EpochNumber::INITIAL,
            position: FeedPosition::ZERO,
        });
    };
    if rows
        .get(1)
        .is_some_and(|previous| previous.position == row.position)
    {
        return Err(ProjectStorageError::InvalidFeed);
    }
    RecordId::try_from(row.entry_id.clone()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    if row.position <= 0 {
        return Err(ProjectStorageError::InvalidFeed);
    }
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(
            u64::try_from(row.epoch).map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?,
        position: FeedPosition::try_from(
            u64::try_from(row.position).map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}

pub(crate) async fn next_feed_cursor(
    transaction: &mut WriteTransaction,
) -> Result<FeedCursor, ProjectStorageError> {
    let admitted = EpochNumber::try_from(transaction.admitted_epoch().value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    let line = latest_line(transaction)
        .await?
        .ok_or(ProjectStorageError::InvalidFeed)?;
    if line.epoch != admitted {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let high = checkpoint(transaction).await?;
    let last = high.position.max(line.inherited_through);
    Ok(FeedCursor {
        epoch: admitted,
        position: last
            .successor()
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
    })
}

pub(crate) async fn ensure_epoch_line(
    transaction: &mut WriteTransaction,
    admission: &RecordAdmission,
) -> Result<(), ProjectStorageError> {
    let epoch = EpochNumber::try_from(transaction.admitted_epoch().value())
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
    if let Some(current) = latest_line(&mut *transaction).await? {
        if current.epoch > epoch {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if current.epoch == epoch {
            return Ok(());
        }
    }
    let inherited = checkpoint(&mut *transaction).await?.position;
    let epoch_text = epoch.get().to_string();
    let id = admission.row_key(ImportRowKind::EpochLine, &[&epoch_text])?;
    let id = id.as_str();
    let number = i64::try_from(epoch.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let inherited = i64::try_from(inherited.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    sqlx_turso::query!(
        "INSERT INTO epoch_lines(line_id, epoch, inherited_through) VALUES (?, ?, ?)",
        id,
        number,
        inherited
    )
    .execute(transaction)
    .await?;
    Ok(())
}

impl ProjectSnapshot {
    pub async fn cursor_validities(
        &mut self,
        cursors: &[FeedCursor],
    ) -> Result<
        std::collections::BTreeMap<(EpochNumber, FeedPosition), CursorValidity>,
        ProjectStorageError,
    > {
        cursor_validities(&mut self.transaction, cursors, self.high_water).await
    }

    pub async fn cursor_validity(
        &mut self,
        cursor: FeedCursor,
    ) -> Result<CursorValidity, ProjectStorageError> {
        cursor_validity(&mut self.transaction, cursor, self.high_water).await
    }

    pub async fn feed_cursor_at(
        &mut self,
        position: FeedPosition,
    ) -> Result<Option<FeedCursor>, ProjectStorageError> {
        cursor_at_position(&mut self.transaction, position).await
    }

    /// Dense feed positions keep their assigning epoch; only zero has no row.
    pub async fn feed_cursor_or_start(
        &mut self,
        position: FeedPosition,
    ) -> Result<FeedCursor, ProjectStorageError> {
        if position == FeedPosition::ZERO {
            return Ok(FeedCursor {
                epoch: self.high_water.epoch,
                position,
            });
        }
        self.feed_cursor_at(position)
            .await?
            .ok_or(ProjectStorageError::InvalidFeed)
    }

    pub async fn epoch_line(
        &mut self,
        epoch: EpochNumber,
    ) -> Result<Option<EpochLine>, ProjectStorageError> {
        let epoch = i64::try_from(epoch.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let row = sqlx_turso::query_as!(StoredEpochLine,
            r#"SELECT line_id AS "line_id!: String", epoch AS "epoch!: i64", inherited_through AS "inherited_through!: i64" FROM epoch_lines WHERE epoch = ?"#,
            epoch
        ).fetch_optional(&mut self.transaction).await?;
        row.map(StoredEpochLine::decode).transpose()
    }
}

impl crate::ProjectWriteTransaction {
    pub async fn cursor_validities(
        &mut self,
        cursors: &[FeedCursor],
    ) -> Result<
        std::collections::BTreeMap<(EpochNumber, FeedPosition), CursorValidity>,
        ProjectStorageError,
    > {
        if cursors.is_empty() {
            return Ok(std::collections::BTreeMap::new());
        }
        let high_water = checkpoint(&mut self.transaction).await?;
        cursor_validities(&mut self.transaction, cursors, high_water).await
    }

    pub async fn cursor_validity(
        &mut self,
        cursor: FeedCursor,
    ) -> Result<CursorValidity, ProjectStorageError> {
        let high_water = checkpoint(&mut self.transaction).await?;
        cursor_validity(&mut self.transaction, cursor, high_water).await
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CursorValidity {
    Valid,
    Superseded { resume_from: FeedCursor },
    Invalid,
}

pub(crate) async fn cursor_at_position<TScope>(
    transaction: &mut TScope,
    position: FeedPosition,
) -> Result<Option<FeedCursor>, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let number = i64::try_from(position.get()).map_err(|_| ProjectStorageError::InvalidFeed)?;
    let rows = sqlx_turso::query!(r#"SELECT entry_id AS "entry_id!: String", epoch AS "epoch!: i64" FROM project_feed WHERE position=? LIMIT 2"#, number)
        .fetch_all(&mut *transaction).await?;
    match rows.as_slice() {
        [] => Ok(None),
        [row] => {
            RecordId::try_from(row.entry_id.clone())
                .map_err(|_| ProjectStorageError::InvalidFeed)?;
            Ok(Some(FeedCursor {
                epoch: EpochNumber::try_from(
                    u64::try_from(row.epoch).map_err(|_| ProjectStorageError::InvalidFeed)?,
                )
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
                position,
            }))
        }
        _ => Err(ProjectStorageError::InvalidFeed),
    }
}

struct StoredCursorFacts {
    cursor_epoch: i64,
    cursor_position: i64,
    retained_count: i64,
    retained_epoch: Option<i64>,
    known_line_id: Option<String>,
    later_line_id: Option<String>,
    later_epoch: Option<i64>,
    inherited_through: Option<i64>,
    resume_count: i64,
    resume_epoch: Option<i64>,
    resume_entry_id: Option<String>,
}
impl StoredCursorFacts {
    fn decode(
        self,
        start: FeedCursor,
    ) -> Result<(FeedCursor, CursorValidity), ProjectStorageError> {
        let cursor = FeedCursor {
            epoch: EpochNumber::try_from(
                u64::try_from(self.cursor_epoch).map_err(|_| ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
            position: FeedPosition::try_from(
                u64::try_from(self.cursor_position)
                    .map_err(|_| ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
        };
        if self.retained_count > 1 {
            return Err(ProjectStorageError::InvalidFeed);
        }
        if cursor.position == FeedPosition::ZERO
            || self.retained_count == 1 && self.retained_epoch == Some(self.cursor_epoch)
        {
            return Ok((cursor, CursorValidity::Valid));
        }
        let Some(known) = self.known_line_id else {
            return Ok((cursor, CursorValidity::Invalid));
        };
        EpochLineId::try_from(known).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let Some(later) = self.later_line_id else {
            return Ok((cursor, CursorValidity::Invalid));
        };
        EpochLineId::try_from(later).map_err(|_| ProjectStorageError::InvalidFeed)?;
        let later_epoch = EpochNumber::try_from(
            u64::try_from(self.later_epoch.ok_or(ProjectStorageError::InvalidFeed)?)
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
        if later_epoch <= cursor.epoch {
            return Err(ProjectStorageError::InvalidFeed);
        }
        let boundary = FeedPosition::try_from(
            u64::try_from(
                self.inherited_through
                    .ok_or(ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?,
        )
        .map_err(|_| ProjectStorageError::InvalidFeed)?;
        let resume_from = if boundary == FeedPosition::ZERO {
            FeedCursor {
                epoch: start.epoch,
                position: FeedPosition::ZERO,
            }
        } else {
            if self.resume_count != 1 {
                return Err(ProjectStorageError::InvalidFeed);
            }
            RecordId::try_from(
                self.resume_entry_id
                    .ok_or(ProjectStorageError::InvalidFeed)?,
            )
            .map_err(|_| ProjectStorageError::InvalidFeed)?;
            FeedCursor {
                epoch: EpochNumber::try_from(
                    u64::try_from(self.resume_epoch.ok_or(ProjectStorageError::InvalidFeed)?)
                        .map_err(|_| ProjectStorageError::InvalidFeed)?,
                )
                .map_err(|_| ProjectStorageError::InvalidFeed)?,
                position: boundary,
            }
        };
        Ok((cursor, CursorValidity::Superseded { resume_from }))
    }
}

pub(crate) async fn cursor_validities<TScope>(
    transaction: &mut TScope,
    cursors: &[FeedCursor],
    high: FeedCursor,
) -> Result<
    std::collections::BTreeMap<(EpochNumber, FeedPosition), CursorValidity>,
    ProjectStorageError,
>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    let keys: std::collections::BTreeSet<_> = cursors
        .iter()
        .map(|cursor| (cursor.epoch, cursor.position))
        .collect();
    if keys.is_empty() {
        return Ok(std::collections::BTreeMap::new());
    }
    let encoded = serde_json::to_string(&keys)?;
    let rows = sqlx_turso::query_file_as!(
        StoredCursorFacts,
        "src/project_cursor_validity_batch.sql",
        encoded
    )
    .fetch_all(&mut *transaction)
    .await?;
    if rows.len() != keys.len() {
        return Err(ProjectStorageError::InvalidFeed);
    }
    let mut results = std::collections::BTreeMap::new();
    for row in rows {
        let (cursor, validity) = row.decode(high)?;
        if !keys.contains(&(cursor.epoch, cursor.position))
            || results
                .insert((cursor.epoch, cursor.position), validity)
                .is_some()
        {
            return Err(ProjectStorageError::InvalidFeed);
        }
    }
    Ok(results)
}

pub(crate) async fn cursor_validity<TScope>(
    transaction: &mut TScope,
    cursor: FeedCursor,
    high: FeedCursor,
) -> Result<CursorValidity, ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    if cursor.position == FeedPosition::ZERO {
        return Ok(CursorValidity::Valid);
    }
    cursor_validities(transaction, std::slice::from_ref(&cursor), high)
        .await?
        .remove(&(cursor.epoch, cursor.position))
        .ok_or(ProjectStorageError::InvalidFeed)
}

pub(crate) async fn require_valid_cursor<TScope>(
    transaction: &mut TScope,
    cursor: FeedCursor,
    high: FeedCursor,
) -> Result<(), ProjectStorageError>
where
    for<'scope> &'scope mut TScope: Executor<'scope, Database = Turso>,
{
    match cursor_validity(transaction, cursor, high).await? {
        CursorValidity::Valid => Ok(()),
        CursorValidity::Superseded { resume_from } => {
            Err(ProjectStorageError::CursorSuperseded { resume_from })
        }
        CursorValidity::Invalid => Err(ProjectStorageError::InvalidCursor),
    }
}
