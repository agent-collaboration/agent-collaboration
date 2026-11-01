//! Debug clock controls observe platform callbacks, separately from business rows.
use collaboration_central_core::RegistryTime;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AutomationClockControl {
    /// Startup composition only. A retained clock takes precedence over this initial value.
    Start {
        now: RegistryTime,
    },
    Advance {
        now: RegistryTime,
    },
    Observe,
    /// At-least-once and early delivery are explicit platform injections.
    InvokeAlarm,
    RemoveAlarm,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationClockObservation {
    pub now: RegistryTime,
    pub next_alarm_ms: Option<i64>,
    pub platform_alarm_before_control_ms: Option<i64>,
    pub callbacks: Vec<AutomationAlarmReceipt>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AutomationAlarmReceipt {
    pub at_ms: i64,
    pub result: AutomationAlarmResult,
    pub next_alarm_ms: Option<i64>,
    pub after_handler_alarm_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum AutomationAlarmResult {
    Completed,
    Failed { message: String },
}
