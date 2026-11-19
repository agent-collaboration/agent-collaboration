//! Verification-only events; no hook or binding exists in a production bundle.
use super::{CentralObject, ObjectHost};
use central_storage::{ReadOutcome, SqlValue, Statement, StoreFailure};
use serde::Serialize;
use worker::{Env, Method, RequestInit, Result};

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum EnrollmentQuery {
    ByEnrollment,
    MachineById,
    ActiveEnrollment,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum ReadResult {
    Read,
    Unavailable,
}
#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) enum RestoreOutcome {
    Admitted,
    Refused,
    Unavailable,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Event {
    EnrollmentRead {
        object_id: String,
        query: EnrollmentQuery,
        outcome: ReadResult,
    },
    RestoreResult {
        object_id: String,
        correlation_id: central_link_protocol::CorrelationId,
        outcome: RestoreOutcome,
    },
    AlarmSnapshot {
        object_id: String,
        correlation_id: central_link_protocol::CorrelationId,
        earliest_due_ms: Option<i64>,
        alarm_ms: Option<i64>,
    },
}

fn enabled(env: &Env) -> Result<bool> {
    worker::js_sys::Reflect::has(
        env,
        &worker::wasm_bindgen::JsValue::from_str("TEST_C2_WITNESS"),
    )
    .map_err(worker::Error::from)
}
async fn record(env: &Env, event: Event) -> Result<()> {
    if !enabled(env)? {
        return Ok(());
    }
    let mut init = RequestInit::new();
    init.with_method(Method::Post)
        .with_body(Some(serde_json::to_string(&event)?.into()));
    let response = env
        .service("TEST_C2_WITNESS")?
        .fetch("https://c2-witness.test/record", Some(init))
        .await?;
    if response.status().as_u16() != 204 {
        return Err(worker::Error::RustError(
            "C2 witness acknowledgement failed".into(),
        ));
    }
    Ok(())
}

pub(super) async fn read_completed(
    env: &Env,
    object_id: String,
    sql: &str,
    result: &std::result::Result<ReadOutcome, StoreFailure>,
) -> Result<()> {
    let Some(query) = classify(sql) else {
        return Ok(());
    };
    record(
        env,
        Event::EnrollmentRead {
            object_id,
            query,
            outcome: if result.is_ok() {
                ReadResult::Read
            } else {
                ReadResult::Unavailable
            },
        },
    )
    .await
}

fn classify(sql: &str) -> Option<EnrollmentQuery> {
    // The independent observer must fail closed when these owning queries change.
    match sql {
        "SELECT a.approval_id,a.request_id,a.label,a.allowed_workspaces,a.state,a.end_reason,a.approved_by_person_id,a.permission_checked,a.base_generation,a.code_expires_at,a.machine_client_id,a.last_grant_at,a.ended_at,a.created_at,a.updated_at,m.machine_id FROM login_machine_approvals a LEFT JOIN login_machines m ON m.machine_id=a.machine_id WHERE a.approval_id=?" => {
            Some(EnrollmentQuery::ByEnrollment)
        }
        "SELECT a.approval_id,a.request_id,a.label,a.allowed_workspaces,a.state,a.end_reason,a.approved_by_person_id,a.permission_checked,a.base_generation,a.code_expires_at,a.machine_client_id,a.last_grant_at,a.ended_at,a.created_at,a.updated_at,m.machine_id FROM login_machine_approvals a LEFT JOIN login_machines m ON m.machine_id=a.machine_id WHERE a.machine_id=? AND a.state='enrolled'" => {
            Some(EnrollmentQuery::ActiveEnrollment)
        }
        "SELECT machine_id,label,state,allowed_workspaces,enrollment_generation,created_at,updated_at FROM login_machines WHERE machine_id=?" => {
            Some(EnrollmentQuery::MachineById)
        }
        _ => None,
    }
}
impl CentralObject {
    pub(super) async fn witness_restored(
        &self,
        correlation_id: &central_link_protocol::CorrelationId,
        outcome: RestoreOutcome,
    ) -> Result<()> {
        record(
            &self.env,
            Event::RestoreResult {
                object_id: self.state.id().to_string(),
                correlation_id: correlation_id.clone(),
                outcome,
            },
        )
        .await
    }
    pub(super) async fn witness_alarm(
        &self,
        correlation_id: &central_link_protocol::CorrelationId,
    ) -> Result<()> {
        if !enabled(&self.env)? {
            return Ok(());
        }
        let rows = self
            .statement_host()
            .read(Statement {
                sql: collaboration_central_core::ENGINE_ALARM_INPUTS,
                args: vec![],
            })
            .await
            .map_err(|_| worker::Error::RustError("C2 due read unavailable".into()))?;
        // The engine's earliest due is the earlier of its on-grid and legacy inputs.
        let due_value = |value: &SqlValue| match value {
            SqlValue::Null => Ok(None),
            SqlValue::Integer(value) if value.get() >= 0 => Ok(Some(value.get())),
            _ => Err(worker::Error::RustError("C2 invalid due value".into())),
        };
        let earliest_due_ms = match (rows.columns.as_slice(), rows.rows.as_slice()) {
            (columns, [row]) if columns == ["on_grid_due", "legacy_due"] => match row.as_slice() {
                [on_grid, legacy] => [due_value(on_grid)?, due_value(legacy)?]
                    .into_iter()
                    .flatten()
                    .min(),
                _ => return Err(worker::Error::RustError("C2 invalid due value".into())),
            },
            _ => return Err(worker::Error::RustError("C2 invalid due row".into())),
        };
        let alarm_ms = self.state.storage().get_alarm().await?;
        if alarm_ms
            .is_some_and(|value| value < 0 || central_storage::SafeInteger::new(value).is_err())
        {
            return Err(worker::Error::RustError("C2 invalid alarm value".into()));
        }
        record(
            &self.env,
            Event::AlarmSnapshot {
                object_id: self.state.id().to_string(),
                correlation_id: correlation_id.clone(),
                earliest_due_ms,
                alarm_ms,
            },
        )
        .await
    }
}
