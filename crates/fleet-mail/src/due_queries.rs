//! The two mail due lists and their controlled alarm inputs.
#[macro_export]
#[doc(hidden)]
macro_rules! __direct_message_rows {
    () => {
        $crate::__machine_rows!(
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
#[macro_export]
#[doc(hidden)]
macro_rules! __system_message_expiry_rows {
    () => {
        $crate::__machine_rows!(
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

#[macro_export]
macro_rules! direct_message_alarm_input {
    () => {
        $crate::__controlled_machine!("direct_message", $crate::__direct_message_rows!())
    };
}

#[macro_export]
macro_rules! system_message_expiry_alarm_input {
    () => {
        $crate::__controlled_machine!(
            "system_message_expiry",
            $crate::__system_message_expiry_rows!()
        )
    };
}

/// Direct messages still queued at their cancel mark (design 2026-11-17).
pub const DIRECT_MESSAGE_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    __direct_message_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);

pub const SYSTEM_MESSAGE_EXPIRY_LIST: &str = concat!(
    "SELECT subject_key,occurrence_ms,effective_due_ms FROM (",
    __system_message_expiry_rows!(),
    ") WHERE effective_due_ms<=?1 ORDER BY effective_due_ms,subject_key LIMIT ?2"
);
