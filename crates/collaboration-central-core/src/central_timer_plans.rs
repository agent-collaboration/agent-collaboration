//! Timer request plans; validated rows live in central_timer_rows.
use crate::central_timer_rows::decode_automations;
use crate::{
    WorkspaceRuntime,
    central_mail_store::{encode, timestamp},
    guarded_statements::{exact, text},
};
use agent_automation::TimingRule;
use central_storage::{GuardedStatement, Statement, StoreFailure};
use fleet_runtime::stored_values::string;
use fleet_runtime::workspace_decode_error::corrupt;
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use workspace_ids::{AutomationId, WakeId};
use workspace_model::Timestamp;
use workspace_model::central_mail as mail;
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn plan_timer_request(
        &self,
        operation: &mail::CentralMailOperation,
        now: Timestamp,
    ) -> Result<
        Result<(Vec<GuardedStatement>, mail::CentralMailAnswer), mail::CentralMailRefusal>,
        StoreFailure,
    > {
        use mail::{
            CentralMailAnswer as Answer, CentralMailOperation as Op, CentralMailRefusal as Refusal,
        };
        let at = || text(timestamp(now));
        let decision = match operation {
            Op::SetWake {
                target,
                due_at,
                text: body,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                let id: WakeId = self.fleet_runtime.mint()?;
                (
                    vec![exact(
                        "INSERT INTO wakes(wake_id,target,due_at,text,state,created_at,updated_at) VALUES (?,?,?,?,'active',?,?)",
                        vec![
                            text(&id),
                            encode(target)?,
                            text(timestamp(*due_at)),
                            text(body.as_str()),
                            at(),
                            at(),
                        ],
                    )],
                    Answer::WakeSet {
                        wake_id: id,
                        due_at: *due_at,
                    },
                )
            }
            Op::CancelWake { wake_id } => {
                let rows = self
                    .statement_host()
                    .read(Statement {
                        sql: "SELECT state FROM wakes WHERE wake_id=?",
                        args: vec![text(wake_id)],
                    })
                    .await?;
                let Some(row) = rows.rows.first() else {
                    return Ok(Err(Refusal::WakeNotFound {
                        wake_id: wake_id.clone(),
                    }));
                };
                let state: mail::WakeState = serde_json::from_value(serde_json::Value::String(
                    string(&rows, row, "state")?.to_owned(),
                ))
                .map_err(|_| corrupt("wakes.state"))?;
                if state == mail::WakeState::Done {
                    return Ok(Err(Refusal::WakeAlreadyFired {
                        wake_id: wake_id.clone(),
                    }));
                }
                let mut statements = if state == mail::WakeState::Disabled {
                    vec![]
                } else {
                    vec![exact(
                        "UPDATE wakes SET state='disabled',updated_at=? WHERE wake_id=? AND state='active'",
                        vec![at(), text(wake_id)],
                    )]
                };
                statements.push(crate::engine::attempt_statements::delete_attempt(
                    &crate::engine::attempt_records::EngineSubject {
                        kind: central_engine_model::MachineKind("wake_timer"),
                        key: central_engine_model::InstanceKey(wake_id.to_string()),
                    },
                ));
                (
                    statements,
                    Answer::WakeCancelled {
                        wake_id: wake_id.clone(),
                    },
                )
            }
            Op::SetAutomation {
                target,
                timing,
                text: body,
            } => {
                if !self.known_session(target).await? {
                    return Ok(Err(Refusal::UnknownSession {
                        target: target.clone(),
                    }));
                }
                let first = match crate::central_mail_time::first_due(timing, now) {
                    Ok(Some(first)) => first,
                    Ok(None) | Err(agent_automation::TimingError::OutOfRange) => {
                        return Ok(Err(Refusal::NoOccurrence));
                    }
                    Err(_) => {
                        return Ok(Err(Refusal::InvalidInput {
                            field: "timing".to_owned(),
                        }));
                    }
                };
                let Some(expires_at) = crate::central_mail_time::lifetime_end(now) else {
                    return Ok(Err(Refusal::TimeOutOfRange {
                        field: "anchorAt".to_owned(),
                    }));
                };
                if first > expires_at {
                    return Ok(Err(Refusal::TimingViolation {
                        field: match timing {
                            TimingRule::At { .. } => "timing.at",
                            TimingRule::After { .. } | TimingRule::Interval { .. } => {
                                "timing.seconds"
                            }
                            TimingRule::Cron { .. } => "timing.expression",
                        }
                        .to_owned(),
                        requirement: mail::TimingRequirement::WithinSevenDays,
                    }));
                }
                let Some(anchor) = crate::central_mail_time::aligned_at_or_after(now, 300) else {
                    return Ok(Err(Refusal::TimeOutOfRange {
                        field: "anchorAt".to_owned(),
                    }));
                };
                let id: AutomationId = self.fleet_runtime.mint()?;
                (
                    vec![exact(
                        "INSERT INTO automations(automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at,updated_at) VALUES (?,?,?,?,?,NULL,?,?,'active',?,?)",
                        vec![
                            text(&id),
                            encode(target)?,
                            encode(timing)?,
                            text(timestamp(anchor)),
                            text(timestamp(expires_at)),
                            text(body.as_str()),
                            text(timestamp(first)),
                            at(),
                            at(),
                        ],
                    )],
                    Answer::AutomationSet {
                        automation_id: id,
                        next_due_at: Some(first),
                        state: mail::AutomationState::Active,
                    },
                )
            }
            Op::DisableAutomation { automation_id } => {
                let rows=self.statement_host().read(Statement{sql:"SELECT automation_id,target,timing_rule,anchor_at,expires_at,evaluated_through_at,text,next_due_at,state,created_at FROM automations WHERE automation_id=?",args:vec![text(automation_id)]}).await?;
                let rows = decode_automations(&rows)?;
                let Some(row) = rows.first() else {
                    return Ok(Err(Refusal::AutomationNotFound {
                        automation_id: automation_id.clone(),
                    }));
                };
                if row.state == mail::AutomationState::Done {
                    return Ok(Err(Refusal::AutomationAlreadyDone {
                        automation_id: automation_id.clone(),
                    }));
                }
                let mut statements = if row.state == mail::AutomationState::Disabled {
                    vec![]
                } else {
                    vec![exact(
                        "UPDATE automations SET state='disabled',updated_at=? WHERE automation_id=? AND state='active'",
                        vec![at(), text(automation_id)],
                    )]
                };
                statements.push(crate::engine::attempt_statements::delete_attempt(
                    &crate::engine::attempt_records::EngineSubject {
                        kind: central_engine_model::MachineKind("automation_timer"),
                        key: central_engine_model::InstanceKey(automation_id.to_string()),
                    },
                ));
                (
                    statements,
                    Answer::AutomationDisabled {
                        automation_id: automation_id.clone(),
                    },
                )
            }
            Op::SendMessage { .. } | Op::SetNoticeCadence { .. } => {
                return Err(corrupt("timer operation"));
            }
        };
        Ok(Ok(decision))
    }
}
