//! The controlled alarm scalar for the notice window whole unit.

#[macro_export]
macro_rules! notice_window_alarm_input {
    () => {
        $crate::__controlled_legacy!(
            "legacy_notice_windows",
            "mail_notification_windows",
            "due_at"
        )
    };
}
