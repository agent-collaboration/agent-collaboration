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
                sql: "SELECT wake_id,target,due_at,text,state,created_at FROM wakes",
                args: vec![],
            })
            .await?;
        let automations = self.host.read(Statement {
            sql: "SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at FROM automations",
            args: vec![],
        }).await?;
        let wakes = decode_wakes(&wakes)?;
        let automations = decode_automations(&automations)?;
        self.validate_notification_due_rows().await?;
        let messages = self
            .host
            .read(Statement {
                sql: "SELECT deliver_after FROM messages WHERE state='queued'",
                args: vec![],
            })
            .await?;
        for row in &messages.rows {
            crate::notification_windows::parse_time(
                crate::workspace_row_codec::string(&messages, row, "deliver_after")?,
                "messages.deliver_after",
            )?;
        }
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
            let messages=self.host.read(Statement{sql:"SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,deliver_after,created_at,updated_at FROM messages WHERE state IN ('queued','ready')",args:vec![]}).await?;
            crate::central_mail_records::decode_messages(&messages)?;
            let mut statements = Vec::new();
            let mut wakes_fired = 0;
            let mut automations_fired = 0;

            for wake in wakes {
                if wake.state != mail::WakeState::Active || wake.due > now {
                    continue;
                }
                let mut message = self.prepare_message_record(
                    mail::MessageSender::Central,
                    wake.target,
                    mail::MessageBody::Wake {
                        wake_id: wake.id.clone(),
                        text: wake.text,
                    },
                    None,
                    now,
                )?;
                message.deliver_after = wake.due;
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
                let next = next.filter(|due| *due <= automation.expires_at);
                if next.is_some_and(|due| !crate::central_mail_time::supported(due)) {
                    return Err(corrupt("automations.next_due_at"));
                }
                let mut message = self.prepare_message_record(
                    mail::MessageSender::Central,
                    automation.target,
                    mail::MessageBody::Automation {
                        automation_id: automation.id.clone(),
                        text: automation.text,
                    },
                    None,
                    now,
                )?;
                message.deliver_after = automation
                    .due
                    .ok_or_else(|| corrupt("automations.next_due_at"))?;
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
            statements.push(crate::guarded_statements::any("UPDATE messages SET state='ready',updated_at=? WHERE state='queued' AND julianday(deliver_after)<=julianday(?)",vec![text(timestamp(now)),text(timestamp(now))]));
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
        #[cfg(feature = "test-support")]
        self.completed_alarms
            .send_modify(|count| *count = count.saturating_add(1));
        Ok(AlarmResult {
            wakes_fired,
            automations_fired,
            windows_fired,
            workspaces_drained,
        })
    }
}
