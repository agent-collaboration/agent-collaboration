//! A creation refusal names the library's durable registration outcome.
use fleet_sync_protocol::RegistrationRefusal;
use fleet_sync_writer::WriterRegistrationOutcome;
use message_board::*;

pub(super) fn registration_rejection(
    outcome: &WriterRegistrationOutcome,
) -> Option<ProjectRegistrationRejection> {
    match outcome {
        WriterRegistrationOutcome::Refused {
            reason: RegistrationRefusal::DatabaseRejected { reason },
        } => Some(ProjectRegistrationRejection {
            code: reason.code.as_str().to_owned(),
            detail: reason.detail.clone(),
        }),
        _ => None,
    }
}
