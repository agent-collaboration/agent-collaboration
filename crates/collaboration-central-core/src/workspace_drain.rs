//! Workspace turn precedes central commit; durable deadlines precede workspace writes.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{alarm_update, applied, timestamp},
    guarded_statements::{exact, text},
    notification_windows::{parse, parse_time},
    workspace_decode_error::corrupt,
    workspace_notice_plans::DrainResult,
    workspace_row_codec::{WorkspaceRows, column, string},
};
use central_storage::{SqlValue, Statement, StatementBatch, StoreFailure};
use workspace_model::{ChangePosition, Timestamp, WorkspaceId};
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn preset_workspace_drain(
        &self,
        workspace: &WorkspaceId,
        now: Timestamp,
    ) -> Result<(), StoreFailure> {
        let due = now
            .checked_add_signed(chrono::Duration::seconds(5))
            .ok_or_else(|| corrupt("drain_due_at"))?;
        let _turn = self.take_central_turn().await;
        self.validated_timers().await?;
        applied(self.host.run_batch(StatementBatch{statements:vec![exact("INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=COALESCE(workspace_drains.drain_due_at,excluded.drain_due_at),updated_at=excluded.updated_at",vec![text(workspace),text(timestamp(due)),text(timestamp(now)),text(timestamp(now))])],alarm:alarm_update()}).await?)
    }
    pub async fn drain_workspace(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        let _turn = self.turns.take(workspace).await;
        self.drain_workspace_held(workspace).await
    }
    pub(crate) async fn drain_workspace_held(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        let outcome = self.commit_workspace_drain(workspace).await;
        if matches!(
            outcome,
            Err(StoreFailure::Unavailable | StoreFailure::AnswerLost)
        ) {
            // Retry ownership is per workspace. A failure never rewrites another row.
            self.retry_workspace_drain(workspace).await?;
        } else if outcome.is_ok() {
            self.drain_retry_seconds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(workspace);
        }
        outcome
    }
    async fn retry_workspace_drain(&self, workspace: &WorkspaceId) -> Result<(), StoreFailure> {
        let now = self.now()?;
        let seconds = {
            let mut retries = self
                .drain_retry_seconds
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = retries.entry(workspace.clone()).or_insert(1);
            let seconds = *next;
            *next = next.saturating_mul(2).min(30);
            seconds
        };
        let due = now
            .checked_add_signed(chrono::Duration::seconds(
                i64::try_from(seconds).map_err(|_| corrupt("drain retry"))?,
            ))
            .ok_or_else(|| corrupt("drain_due_at"))?;
        let _turn = self.take_central_turn().await;
        self.validated_timers().await?;
        applied(self.host.run_batch(StatementBatch{statements:vec![exact("UPDATE workspace_drains SET drain_due_at=?,updated_at=? WHERE workspace_id=?",vec![text(timestamp(due)),text(timestamp(now)),text(workspace)])],alarm:alarm_update()}).await?)
    }
    async fn commit_workspace_drain(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        let cursor = self
            .host
            .read(Statement {
                sql: "SELECT drained_through_position FROM workspace_drains WHERE workspace_id=?",
                args: vec![text(workspace)],
            })
            .await?;
        let Some(row) = cursor.rows.first() else {
            return Ok(DrainResult::default());
        };
        let through: ChangePosition = match column(&cursor, row, "drained_through_position")? {
            SqlValue::Integer(value) => u64::try_from(value.get())
                .ok()
                .and_then(|value| ChangePosition::try_from(value).ok())
                .ok_or_else(|| corrupt("workspace_drains.drained_through_position"))?,
            _ => return Err(corrupt("workspace_drains.drained_through_position")),
        };
        let rows = WorkspaceRows::decode(
            self.read_workspace(
                workspace,
                Statement {
                    sql: include_str!("workspace_sql/drain.sql"),
                    args: vec![position_value(through)?],
                },
            )
            .await?,
        )?;
        if rows.high_water < through {
            return Err(corrupt("workspace_drains.drained_through_position"));
        }
        let now = self.now()?;
        let _central = self.take_central_turn().await;
        self.validated_timers().await?;
        let (mut statements, result) = self.plan_workspace_notices(workspace, &rows, now).await?;
        statements.push(exact("UPDATE workspace_drains SET drained_through_position=?,drain_due_at=NULL,updated_at=? WHERE workspace_id=? AND drained_through_position=?",vec![position_value(rows.high_water)?,text(timestamp(now)),text(workspace),position_value(through)?]));
        #[cfg(feature = "test-support")]
        self.pauses
            .reached(workspace, crate::PausePoint::BeforeDrainCommit)
            .await;
        applied(
            self.host
                .run_batch(StatementBatch {
                    statements,
                    alarm: alarm_update(),
                })
                .await?,
        )?;
        #[cfg(feature = "test-support")]
        {
            self.pauses
                .reached(workspace, crate::PausePoint::AfterDrainCommit)
                .await;
            self.pauses
                .reached(workspace, crate::PausePoint::AfterNoticesCommitted)
                .await;
        }
        Ok(result)
    }
    pub(crate) async fn drain_due_workspaces(&self, now: Timestamp) -> Result<usize, StoreFailure> {
        let rows=self.host.read(Statement{sql:"SELECT workspace_id,drain_due_at FROM workspace_drains WHERE drain_due_at IS NOT NULL",args:vec![]}).await?;
        let mut workspaces = vec![];
        for row in &rows.rows {
            let workspace: WorkspaceId = parse(
                string(&rows, row, "workspace_id")?,
                "workspace_drains.workspace_id",
            )?;
            let due: Timestamp = parse_time(
                string(&rows, row, "drain_due_at")?,
                "workspace_drains.drain_due_at",
            )?;
            if due <= now {
                workspaces.push(workspace);
            }
        }
        let mut count = 0;
        let mut failure = None;
        for workspace in workspaces {
            match self.drain_workspace(&workspace).await {
                Ok(_) => count += 1,
                Err(error) => {
                    failure = Some(error);
                }
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(count)
    }
}

fn position_value(position: ChangePosition) -> Result<SqlValue, StoreFailure> {
    let value = i64::try_from(position.get()).map_err(|_| corrupt("position"))?;
    Ok(SqlValue::Integer(
        value.try_into().map_err(|_| corrupt("position"))?,
    ))
}
