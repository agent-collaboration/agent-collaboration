//! One native session identity owns both read and handled notice cursors.
use super::{
    ClientSessionState, FleetCache, NoticePosition, RouterLocalStorageError, Utc, invalid,
    parse_u64, validate_id, validate_times,
};

pub(super) struct StoredSession {
    pub state: ClientSessionState,
    pub handled_through: NoticePosition,
}

pub(super) async fn read_session(
    connection: &mut sqlx_turso::TursoConnection,
) -> Result<Option<StoredSession>, RouterLocalStorageError> {
    let rows = sqlx_turso::query!(r#"SELECT session_id AS "session_id!: String", notice_cursor AS "notice_cursor!: String", handled_through AS "handled_through!: String", created_at AS "created_at!: String", updated_at AS "updated_at!: String" FROM fleet_session"#)
        .fetch_all(connection).await?;
    if rows.len() > 1 {
        return Err(invalid("fleetSession.singleton"));
    }
    rows.into_iter()
        .next()
        .map(|row| {
            validate_id(&row.session_id, "fleetSession.sessionId")?;
            validate_times(&row.created_at, &row.updated_at)?;
            Ok(StoredSession {
                state: ClientSessionState {
                    session_id: fleet_sync_protocol::SessionId::new(
                        uuid::Uuid::parse_str(&row.session_id)
                            .map_err(|_| invalid("fleetSession.sessionId"))?,
                    )
                    .map_err(|_| invalid("fleetSession.sessionId"))?,
                    notice_cursor: NoticePosition::new(parse_u64(
                        &row.notice_cursor,
                        "fleetSession.noticeCursor",
                    )?),
                },
                handled_through: NoticePosition::new(parse_u64(
                    &row.handled_through,
                    "machineNoticeProgress",
                )?),
            })
        })
        .transpose()
}

pub(super) async fn insert_session(
    connection: &mut sqlx_turso::TursoConnection,
    state: &ClientSessionState,
    handled: NoticePosition,
) -> Result<(), RouterLocalStorageError> {
    let session = state.session_id.to_string();
    let cursor = state.notice_cursor.value().to_string();
    let handled = handled.value().to_string();
    let now = Utc::now().to_rfc3339();
    sqlx_turso::query!("INSERT INTO fleet_session(session_id, notice_cursor, handled_through, created_at, updated_at) VALUES (?, ?, ?, ?, ?)", session, cursor, handled, now, now).execute(connection).await?;
    Ok(())
}

impl FleetCache {
    pub async fn machine_session_state(
        &self,
    ) -> Result<Option<ClientSessionState>, RouterLocalStorageError> {
        Ok(read_session(&mut *self.database.pool.acquire().await?)
            .await?
            .map(|row| row.state))
    }
    pub async fn machine_notice_progress(&self) -> Result<NoticePosition, RouterLocalStorageError> {
        Ok(read_session(&mut *self.database.pool.acquire().await?)
            .await?
            .map_or(NoticePosition::new(0), |row| row.handled_through))
    }
    pub async fn save_machine_session_state(
        &self,
        state: &ClientSessionState,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        if read_session(&mut transaction).await?.is_some() {
            let session = state.session_id.to_string();
            let cursor = state.notice_cursor.value().to_string();
            let now = Utc::now().to_rfc3339();
            // Native session renewal does not discard machine-owned handled progress.
            sqlx_turso::query!(
                "UPDATE fleet_session SET session_id = ?, notice_cursor = ?, updated_at = ?",
                session,
                cursor,
                now
            )
            .execute(&mut *transaction)
            .await?;
        } else {
            insert_session(&mut transaction, state, NoticePosition::new(0)).await?;
        }
        transaction.commit().await?;
        Ok(())
    }
    pub async fn save_machine_notice_progress(
        &self,
        position: NoticePosition,
    ) -> Result<(), RouterLocalStorageError> {
        let _writer = self.database.writer.lock().await;
        let mut transaction = self.database.pool.begin_with("BEGIN IMMEDIATE").await?;
        if read_session(&mut transaction).await?.is_none() {
            return Err(invalid("machineNoticeProgress.sessionMissing"));
        }
        let handled = position.value().to_string();
        let now = Utc::now().to_rfc3339();
        sqlx_turso::query!(
            "UPDATE fleet_session SET handled_through = ?, updated_at = ?",
            handled,
            now
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}
