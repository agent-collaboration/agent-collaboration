//! The central alarm validates and fires due work under one serialized turn.
use crate::{
    ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_store::{alarm_update, applied, insert_message, timestamp},
    central_timer_plans::{AutomationRow, WakeRow, decode_automations, decode_wakes},
    guarded_statements::{exact, text},
    workspace_decode_error::corrupt,
};
use central_storage::{SqlValue, Statement, StatementBatch, StoreFailure};
use collaboration_protocol::central_mail as mail;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AlarmResult {
    pub wakes_fired: usize,
    pub automations_fired: usize,
    pub windows_fired: usize,
    pub workspaces_drained: usize,
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    async fn alarm_organization_is_provisioned(&self) -> Result<bool, StoreFailure> {
        match crate::prepare_central_alarm(self.host.as_ref()).await? {
            crate::CentralOrganizationIdentity::Unprovisioned => Ok(false),
            crate::CentralOrganizationIdentity::Stamped { organization_id }
                if organization_id == self.organization =>
            {
                Ok(true)
            }
            crate::CentralOrganizationIdentity::Stamped { .. } => {
                Err(corrupt("organization_identity.id"))
            }
        }
    }
    pub(crate) async fn validated_timers(
        &self,
    ) -> Result<(Vec<WakeRow>, Vec<AutomationRow>), StoreFailure> {
        let wakes = self
            .host
            .read(Statement {
                sql: "SELECT wake_id,target,due_at,text,state FROM wakes",
                args: vec![],
            })
            .await?;
        let automations = self.host.read(Statement {
            sql: "SELECT automation_id,target,timing_rule,anchor_at,evaluated_through_at,text,next_due_at,state FROM automations",
            args: vec![],
        }).await?;
        let wakes = decode_wakes(&wakes)?;
        let automations = decode_automations(&automations)?;
        self.validate_notification_due_rows().await?;
        Ok((wakes, automations))
    }

    pub async fn recompute_alarm(&self) -> Result<(), StoreFailure> {
        let _turn = self.take_central_turn().await;
        if !self.alarm_organization_is_provisioned().await? {
            return Ok(());
        }
        self.validated_timers().await?;
        applied(
            self.host
                .run_batch(StatementBatch {
                    statements: vec![],
                    alarm: alarm_update(),
                })
                .await?,
        )
    }

    pub async fn on_alarm(&self) -> Result<AlarmResult, StoreFailure> {
        let (now, wakes_fired, automations_fired, windows_fired) = {
            let _turn = self.take_central_turn().await;
            if !self.alarm_organization_is_provisioned().await? {
                return Ok(AlarmResult::default());
            }
            let now = self.now()?;
            let (wakes, automations) = self.validated_timers().await?;
            let mut statements = Vec::new();
            let mut wakes_fired = 0;
            let mut automations_fired = 0;

            for wake in wakes {
                if wake.state != mail::WakeState::Active || wake.due > now {
                    continue;
                }
                let message = self.queued_message(
                    mail::MessageSender::Central,
                    wake.target,
                    mail::MessageBody::Wake {
                        wake_id: wake.id.clone(),
                        text: wake.text,
                    },
                    None,
                    now,
                )?;
                statements.push(insert_message(&message)?);
                statements.push(exact(
                    "UPDATE wakes SET state='done',updated_at=? WHERE wake_id=? AND state='active'",
                    vec![text(timestamp(now)), text(&wake.id)],
                ));
                wakes_fired += 1;
            }

            for automation in automations {
                if automation
                    .due
                    .zip(automation.evaluated)
                    .is_some_and(|(due, evaluated)| due <= evaluated)
                {
                    return Err(corrupt("automations.next_due_at"));
                }
                if automation.state != mail::AutomationState::Active
                    || automation.due.is_none_or(|due| due > now)
                {
                    continue;
                }
                let next = automation
                    .timing
                    .next_due(automation.anchor, Some(now))
                    .map_err(|_| corrupt("automations.timing_rule"))?;
                if next.is_some_and(|due| !crate::central_mail_time::supported(due)) {
                    return Err(corrupt("automations.next_due_at"));
                }
                let message = self.queued_message(
                    mail::MessageSender::Central,
                    automation.target,
                    mail::MessageBody::Automation {
                        automation_id: automation.id.clone(),
                        text: automation.text,
                    },
                    None,
                    now,
                )?;
                statements.push(insert_message(&message)?);
                statements.push(exact(
                    "UPDATE automations SET evaluated_through_at=?,next_due_at=?,state=?,updated_at=? WHERE automation_id=? AND state='active'",
                    vec![
                        text(timestamp(now)),
                        next.map_or(SqlValue::Null, |due| text(timestamp(due))),
                        text(if next.is_some() { "active" } else { "done" }),
                        text(timestamp(now)),
                        text(&automation.id),
                    ],
                ));
                automations_fired += 1;
            }
            let (window_statements, windows_fired) = self.plan_due_windows(now).await?;
            statements.extend(window_statements);
            #[cfg(feature = "test-support")]
            self.pauses
                .reached_central(crate::PausePoint::BeforeTimerCommit)
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
            self.pauses
                .reached_central(crate::PausePoint::AfterTimerCommit)
                .await;
            (now, wakes_fired, automations_fired, windows_fired)
        };
        let workspaces_drained = self.drain_due_workspaces(now).await?;
        Ok(AlarmResult {
            wakes_fired,
            automations_fired,
            windows_fired,
            workspaces_drained,
        })
    }
}
