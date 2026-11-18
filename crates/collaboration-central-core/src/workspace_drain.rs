//! Workspace turn precedes central commit; durable deadlines precede workspace writes.
use crate::engine::legacy_units::LegacyUnitOutcome;
use crate::{WorkspaceRuntime, central_mail_store::{alarm_update, applied, timestamp}, guarded_statements::{exact, text}, notification_windows::{parse, parse_time}, workspace_decode_error::corrupt, workspace_notice_plans::DrainResult, workspace_row_codec::{WorkspaceRows, column, string}};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use central_storage::{SqlValue, Statement, StatementBatch, StoreFailure};
use workspace_model::{ChangePosition, Timestamp, WorkspaceId};
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn preset_workspace_drain(
        &self,
        workspace: &WorkspaceId,
        now: Timestamp,
    ) -> Result<(), StoreFailure> {
        let due = now
            .checked_add_signed(chrono::Duration::seconds(5))
            .ok_or_else(|| corrupt("drain_due_at"))?;
        let _turn = self.take_central_turn().await;

        applied(self.host.run_batch(StatementBatch{statements:vec![exact("INSERT INTO workspace_drains(workspace_id,drained_through_position,drain_due_at,created_at,updated_at) VALUES (?,0,?,?,?) ON CONFLICT(workspace_id) DO UPDATE SET drain_due_at=COALESCE(workspace_drains.drain_due_at,excluded.drain_due_at),updated_at=excluded.updated_at",vec![text(workspace),text(timestamp(due)),text(timestamp(now)),text(timestamp(now))])],alarm:alarm_update()}).await?)
    }
    pub async fn drain_workspace(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, StoreFailure> {
        let _turn = self.turns.take(workspace).await;
        self.drain_workspace_held(workspace)
            .await
            .map_err(LegacyUnitOutcome::into_request_failure)
    }
    pub(crate) async fn drain_workspace_held(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, LegacyUnitOutcome> {
        self.commit_workspace_drain(workspace).await
    }

    /// A request owns one workspace's immediate drain, not the whole unit's
    /// success. Its durable answer survives a failed notice delivery.
    pub(crate) async fn drain_after_request(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<(), StoreFailure> {
        use crate::engine::{
            attempt_records::EngineSubject, attempt_statements::CONTROL_WRITE,
            control_gate::ControlGate, failures::FailureOutcome, store_adapter::SubjectVersion,
        };
        use central_engine_model::{InstanceKey, MachineKind, RetirementThreshold};
        let subject = EngineSubject {
            kind: MachineKind("legacy_workspace_drains"),
            key: InstanceKey("all".into()),
        };
        match crate::engine::port::Engine::new(self)
            .control_gate(&subject, RetirementThreshold::NOTICE_UNITS)
            .await
        {
            ControlGate::Run(_) => {}
            ControlGate::Skip | ControlGate::Uncertain => return Ok(()),
            ControlGate::CentralDown(failure) => {
                crate::engine::port::Engine::new(self).observe_engine_store_down(&subject, failure);
                return Ok(());
            }
        }
        #[cfg(feature = "test-support")]
        self.count_legacy_invocation(|count| count.request_drains += 1);
        match self.drain_workspace_held(workspace).await {
            Ok(_) => Ok(()),
            Err(LegacyUnitOutcome::CentralDown(failure)) => {
                crate::engine::port::Engine::new(self).observe_engine_store_down(&subject, failure);
                Ok(())
            }
            Err(outcome) => {
                let outcome = outcome.in_workspace(workspace);
                if let Some(class) = outcome.failure_class()
                    && let FailureOutcome::StoreDown(failure) =
                        crate::engine::port::Engine::new(self)
                            .record_failure_naming(
                                &subject,
                                CONTROL_WRITE,
                                &SubjectVersion(vec![]),
                                class,
                                RetirementThreshold::NOTICE_UNITS,
                                outcome.affected_workspaces(),
                            )
                            .await
                {
                    crate::engine::port::Engine::new(self)
                        .observe_engine_store_down(&subject, failure);
                }
                Ok(())
            }
        }
    }
    async fn commit_workspace_drain(
        &self,
        workspace: &WorkspaceId,
    ) -> Result<DrainResult, LegacyUnitOutcome> {
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
            _ => return Err(corrupt("workspace_drains.drained_through_position").into()),
        };
        let rows = WorkspaceRows::decode(
            self.read_workspace(
                workspace,
                Statement {
                    sql: include_str!("workspace_sql/drain.sql"),
                    args: vec![position_value(through)?],
                },
            )
            .await
            .map_err(|failure| match failure {
                StoreFailure::Unavailable | StoreFailure::AlarmUnpublishable { .. } => {
                    LegacyUnitOutcome::RemoteUnavailable {
                        workspaces: vec![workspace.clone()],
                    }
                }
                failure @ (StoreFailure::Corrupt { .. } | StoreFailure::AnswerLost) => {
                    failure.into()
                }
            })?,
        )?;
        if rows.high_water < through {
            return Err(corrupt("workspace_drains.drained_through_position").into());
        }
        let now = self.now()?;
        let _central = self.take_central_turn().await;

        let (mut statements, result) = self.plan_workspace_notices(workspace, &rows, now).await?;
        statements.push(exact("UPDATE workspace_drains SET drained_through_position=?,drain_due_at=NULL,updated_at=? WHERE workspace_id=? AND drained_through_position=?",vec![position_value(rows.high_water)?,text(timestamp(now)),text(workspace),position_value(through)?]));
        #[cfg(feature = "test-support")]
        self.pauses
            .reached(workspace, crate::PausePoint::BeforeDrainCommit)
            .await;
        LegacyUnitOutcome::batch(
            self.host
                .run_batch(StatementBatch {
                    statements,
                    alarm: alarm_update(),
                })
                .await,
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
    pub(crate) async fn drain_due_workspaces(
        &self,
        now: Timestamp,
    ) -> Result<usize, LegacyUnitOutcome> {
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
        let mut outcome = LegacyUnitOutcome::Done;
        for workspace in workspaces {
            // P5 change 6: a workspace whose turn another request holds is skipped
            // for this batch, not a failure; its drain_due_at stays, and the holder's
            // own drain or a later batch drains it.
            let Some(_turn) = self.turns.try_take(&workspace) else {
                continue;
            };
            match self.commit_workspace_drain(&workspace).await {
                Ok(_) => count += 1,
                Err(error) => {
                    outcome = outcome.combine(error.in_workspace(&workspace));
                    if matches!(outcome, LegacyUnitOutcome::CentralDown(_)) {
                        break;
                    }
                }
            }
        }
        // The unit succeeds when every workspace is Done, Superseded or Uncertain
        // (P5); committed workspaces stay counted.
        match outcome {
            LegacyUnitOutcome::Done
            | LegacyUnitOutcome::Superseded
            | LegacyUnitOutcome::Uncertain => Ok(count),
            failure @ (LegacyUnitOutcome::RemoteUnavailable { .. }
            | LegacyUnitOutcome::Corrupt { .. }
            | LegacyUnitOutcome::Rejected { .. }
            | LegacyUnitOutcome::ConstraintViolated { .. }
            | LegacyUnitOutcome::CentralDown(_)) => Err(failure),
        }
    }
}

fn position_value(position: ChangePosition) -> Result<SqlValue, StoreFailure> {
    let value = i64::try_from(position.get()).map_err(|_| corrupt("position"))?;
    Ok(SqlValue::Integer(
        value.try_into().map_err(|_| corrupt("position"))?,
    ))
}
