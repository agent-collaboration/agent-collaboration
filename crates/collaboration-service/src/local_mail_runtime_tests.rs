//! Actual Local intake, SDK and native owner; only the endpoint answer and clock stand in.
#[path = "local_mail_admission_tests.rs"]
mod admission;
#[path = "local_cancel_crash_tests.rs"]
mod crash;
#[path = "frozen_cancel_settlement_tests.rs"]
mod frozen_settlement;

#[path = "local_mail_readiness_tests.rs"]
mod readiness;

#[path = "local_cancel_report_tests.rs"]
mod report_admission;
