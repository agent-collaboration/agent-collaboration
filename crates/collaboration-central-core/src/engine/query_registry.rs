//! The closed registry owns SQL literals. Time and caps are bound values; no
//! statement is generated or leaked to acquire a static lifetime.

use fleet_engine::{controlled_legacy, controlled_machine, machine_rows};

macro_rules! direct_message_rows {
    () => {
        machine_rows!(
            "mail_messages",
            "message_id",
            "s.cancel_due_at",
            "direct_message",
            "'queued'",
            "'delivered','cancelled','failed'",
            "s.state='queued' AND s.cancel_due_at IS NOT NULL"
        )
    };
}
macro_rules! system_message_expiry_rows {
    () => {
        machine_rows!(
            "mail_messages",
            "message_id",
            "s.delivery_window_ends_at",
            "system_message_expiry",
            "'queued'",
            "'delivered','cancelled','failed'",
            "s.state='queued' AND s.message_kind IN ('notice','deliveryOutcome') AND s.delivery_window_ends_at IS NOT NULL"
        )
    };
}
macro_rules! grid_inputs {
    () => {
        concat!(
            "COALESCE(",
            controlled_machine!(
                "wake_timer",
                machine_rows!("automation_wakes", "wake_id", "s.due_at", "wake_timer")
            ),
            ",9223372036855),",
            "COALESCE(",
            controlled_machine!(
                "automation_timer",
                machine_rows!(
                    "automation_schedules",
                    "automation_id",
                    "s.next_due_at",
                    "automation_timer"
                )
            ),
            ",9223372036855)",
            ",COALESCE(",
            controlled_machine!("direct_message", direct_message_rows!()),
            ",9223372036855),COALESCE(",
            controlled_machine!("system_message_expiry", system_message_expiry_rows!()),
            ",9223372036855)"
        )
    };
}
#[cfg(feature = "test-support")]
macro_rules! probe_rows {
    () => {
        machine_rows!(
            "engine_probe_items",
            "item_id",
            "s.due_at",
            "engine_probe",
            "'waiting'",
            "'fired','stopped'",
            "s.machine_kind='engine_probe' AND s.due_at IS NOT NULL"
        )
    };
}
/// The second test table (V5.4): a timed kind with no machine and no due list,
/// registered only so the alarm-inputs query runs with six kinds.
#[cfg(feature = "test-support")]
macro_rules! alarm_only_rows {
    () => {
        machine_rows!(
            "engine_alarm_only_items",
            "item_id",
            "s.due_at",
            "engine_alarm_only",
            "'waiting'",
            "'fired','stopped'",
            "s.due_at IS NOT NULL"
        )
    };
}
#[cfg(feature = "test-support")]
macro_rules! all_grid_inputs {
    () => {
        concat!(
            grid_inputs!(),
            ",COALESCE(",
            controlled_machine!("engine_probe", probe_rows!()),
            ",9223372036855),COALESCE(",
            controlled_machine!("engine_alarm_only", alarm_only_rows!()),
            ",9223372036855)"
        )
    };
}
#[cfg(not(feature = "test-support"))]
macro_rules! all_grid_inputs {
    () => {
        grid_inputs!()
    };
}
pub(crate) const WAKE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    machine_rows!("automation_wakes", "wake_id", "s.due_at", "wake_timer"),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);
pub(crate) const AUTOMATION_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    machine_rows!(
        "automation_schedules",
        "automation_id",
        "s.next_due_at",
        "automation_timer"
    ),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

/// Direct messages still queued at their cancel mark (design 2026-11-17).
pub(crate) const DIRECT_MESSAGE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    direct_message_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

pub(crate) const SYSTEM_MESSAGE_EXPIRY_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    system_message_expiry_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

pub(crate) const ALARM_INPUTS: &str = concat!(
    "SELECT CASE WHEN (SELECT COUNT(*) FROM signin_organization_identity)=1 THEN NULLIF(min(",
    all_grid_inputs!(),
    "),9223372036855) ELSE NULL END AS on_grid_due,",
    "CASE WHEN (SELECT COUNT(*) FROM signin_organization_identity)=1 THEN NULLIF(min(COALESCE(",
    controlled_legacy!(
        "legacy_notice_windows",
        "mail_notification_windows",
        "due_at"
    ),
    ",9223372036855),COALESCE(",
    controlled_legacy!(
        "legacy_workspace_drains",
        "mail_workspace_drains",
        "drain_due_at"
    ),
    ",9223372036855)),9223372036855) ELSE NULL END AS legacy_due"
);

pub(crate) fn alarm_update() -> central_storage::AlarmUpdate {
    central_storage::AlarmUpdate::SetEngineFromQuery {
        query: central_engine_model::next_due_sql(ALARM_INPUTS),
    }
}

#[cfg(test)]
#[path = "query_registry_test.rs"]
mod tests;

#[cfg(feature = "test-support")]
pub(crate) const PROBE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    probe_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);
