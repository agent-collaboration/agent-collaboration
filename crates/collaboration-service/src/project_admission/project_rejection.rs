//! A creation refusal names the library's durable registration outcome.
use fleet_project_writer::WriterRegistrationOutcome;
use fleet_protocol::RegistrationRefusal;
use message_board::ProjectRegistrationRejection;

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
