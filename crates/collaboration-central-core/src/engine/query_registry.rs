//! The closed registry owns SQL literals. Time and caps are bound values; no
//! statement is generated or leaked to acquire a static lifetime.

macro_rules! time_ms {
    ($column:literal) => {
        concat!(
            "CASE WHEN unixepoch(",
            $column,
            ") IS NULL OR abs(unixepoch(",
            $column,
            ")*1000+CAST(substr(strftime('%f',",
            $column,
            "),4,3) AS INTEGER))>9223372036854 THEN 0 ELSE unixepoch(",
            $column,
            ")*1000+CAST(substr(strftime('%f',",
            $column,
            "),4,3) AS INTEGER) END"
        )
    };
}
macro_rules! machine_rows {
    ($table:literal, $key:literal, $due:literal, $kind:literal) => {
        machine_rows!($table,$key,$due,$kind,"'active'","'done','disabled'","1")
    };
    ($table:literal, $key:literal, $due:literal, $kind:literal, $active:literal, $terminal:literal, $predicate:literal) => { concat!(
        "SELECT s.", $key, " AS subject_key,", time_ms!($due), " AS occurrence_ms,",
        "CASE WHEN a.attempt_id IS NOT NULL THEN CASE WHEN a.state='failing' AND a.last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ",
        time_ms!("a.next_try_at"), " ELSE 0 END WHEN s.state!=",$active," THEN 0 ELSE ", time_ms!($due),
        " END AS effective_due_ms FROM ", $table, " s LEFT JOIN central_work_attempts a ON a.machine_kind='", $kind,
        "' AND a.subject_key=s.", $key, " WHERE s.state NOT IN (",$terminal,") AND (",$predicate,") AND NOT(COALESCE(a.state,'')='retired' AND a.last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable'))"
    ) };
}
macro_rules! grid_inputs {
    () => {
        concat!(
            "COALESCE(",
            controlled_machine!(
                "wake_timer",
                machine_rows!("wakes", "wake_id", "s.due_at", "wake_timer")
            ),
            ",9223372036855),",
            "COALESCE(",
            controlled_machine!(
                "automation_timer",
                machine_rows!(
                    "automations",
                    "automation_id",
                    "s.next_due_at",
                    "automation_timer"
                )
            ),
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
#[cfg(feature = "test-support")]
macro_rules! all_grid_inputs {
    () => {
        concat!(
            grid_inputs!(),
            ",COALESCE(",
            controlled_machine!("engine_probe", probe_rows!()),
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
macro_rules! controlled_machine {
    ($kind:literal, $rows:expr) => { concat!(
        "CASE WHEN EXISTS(SELECT 1 FROM central_work_attempts WHERE machine_kind='engine_source' AND subject_key='", $kind,
        "') THEN (SELECT CASE WHEN state='retired' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN NULL WHEN state='failing' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ", time_ms!("next_try_at"),
        " ELSE 0 END FROM central_work_attempts WHERE machine_kind='engine_source' AND subject_key='", $kind,
        "') ELSE (SELECT MIN(effective_due_ms) FROM (", $rows, ")) END"
    ) };
}
macro_rules! controlled_legacy {
    ($kind:literal, $table:literal, $due:literal) => {
        concat!(
            "CASE WHEN EXISTS(SELECT 1 FROM central_work_attempts WHERE machine_kind='",
            $kind,
            "' AND subject_key='all') THEN ",
            "(SELECT CASE WHEN state='retired' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN NULL WHEN state='failing' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ",
            time_ms!("next_try_at"),
            " ELSE 0 END FROM central_work_attempts WHERE machine_kind='",
            $kind,
            "' AND subject_key='all') ELSE ",
            "(SELECT MIN(",
            time_ms!($due),
            ") FROM ",
            $table,
            " WHERE ",
            $due,
            " IS NOT NULL) END"
        )
    };
}

pub(crate) const WAKE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    machine_rows!("wakes", "wake_id", "s.due_at", "wake_timer"),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);
pub(crate) const AUTOMATION_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    machine_rows!(
        "automations",
        "automation_id",
        "s.next_due_at",
        "automation_timer"
    ),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

pub(crate) const ALARM_INPUTS: &str = concat!(
    "SELECT CASE WHEN (SELECT COUNT(*) FROM organization_identity)=1 THEN NULLIF(min(",
    all_grid_inputs!(),
    "),9223372036855) ELSE NULL END AS on_grid_due,",
    "CASE WHEN (SELECT COUNT(*) FROM organization_identity)=1 THEN NULLIF(min(COALESCE(",
    controlled_legacy!("legacy_notice_windows", "notification_windows", "due_at"),
    ",9223372036855),COALESCE(",
    controlled_legacy!(
        "legacy_workspace_drains",
        "workspace_drains",
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
