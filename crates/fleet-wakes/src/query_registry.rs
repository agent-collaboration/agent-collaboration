#[doc(hidden)]
pub use fleet_engine::{
    controlled_machine as __controlled_machine, machine_rows as __machine_rows,
};

#[macro_export]
#[doc(hidden)]
macro_rules! __wake_rows {
    () => {
        $crate::__machine_rows!("automation_wakes", "wake_id", "s.due_at", "wake_timer")
    };
}

#[macro_export]
macro_rules! wake_alarm_input {
    () => {
        $crate::__controlled_machine!("wake_timer", $crate::__wake_rows!())
    };
}

pub const WAKE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    __wake_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

#[cfg(test)]
#[path = "query_registry_test.rs"]
mod tests;
