//! Generic due-list and alarm-input SQL shapes.
#[macro_export]
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
#[macro_export]
macro_rules! machine_rows {
    ($table:literal, $key:literal, $due:literal, $kind:literal) => {
        $crate::machine_rows!($table,$key,$due,$kind,"'active'","'done','disabled'","1")
    };
    ($table:literal, $key:literal, $due:literal, $kind:literal, $active:literal, $terminal:literal, $predicate:literal) => { concat!(
        "SELECT s.", $key, " AS subject_key,", $crate::time_ms!($due), " AS occurrence_ms,",
        "CASE WHEN a.attempt_id IS NOT NULL THEN CASE WHEN a.state='failing' AND a.last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ",
        $crate::time_ms!("a.next_try_at"), " ELSE 0 END WHEN s.state!=",$active," THEN 0 ELSE ", $crate::time_ms!($due),
        " END AS effective_due_ms FROM ", $table, " s LEFT JOIN engine_work_attempts a ON a.machine_kind='", $kind,
        "' AND a.subject_key=s.", $key, " WHERE s.state NOT IN (",$terminal,") AND (",$predicate,") AND NOT(COALESCE(a.state,'')='retired' AND a.last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable'))"
    ) };
}
#[macro_export]
macro_rules! controlled_machine {
    ($kind:literal, $rows:expr) => { concat!(
        "CASE WHEN EXISTS(SELECT 1 FROM engine_work_attempts WHERE machine_kind='engine_source' AND subject_key='", $kind,
        "') THEN (SELECT CASE WHEN state='retired' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN NULL WHEN state='failing' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ", $crate::time_ms!("next_try_at"),
        " ELSE 0 END FROM engine_work_attempts WHERE machine_kind='engine_source' AND subject_key='", $kind,
        "') ELSE (SELECT MIN(effective_due_ms) FROM (", $rows, ")) END"
    ) };
}
#[macro_export]
macro_rules! controlled_legacy {
    ($kind:literal, $table:literal, $due:literal) => {
        concat!(
            "CASE WHEN EXISTS(SELECT 1 FROM engine_work_attempts WHERE machine_kind='",
            $kind,
            "' AND subject_key='all') THEN ",
            "(SELECT CASE WHEN state='retired' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN NULL WHEN state='failing' AND last_failure IN ('corrupt_row','constraint_violated','statement_rejected','refused','other_store_unavailable') THEN ",
            $crate::time_ms!("next_try_at"),
            " ELSE 0 END FROM engine_work_attempts WHERE machine_kind='",
            $kind,
            "' AND subject_key='all') ELSE ",
            "(SELECT MIN(",
            $crate::time_ms!($due),
            ") FROM ",
            $table,
            " WHERE ",
            $due,
            " IS NOT NULL) END"
        )
    };
}
