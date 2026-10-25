//! Keep typed admin outcomes and preview observation labels visible to the owner.
use super::*;
use std::io::Write;

pub(super) const UNSENT_NOTICE: &str = "Anything the holder had not sent will not be carried over.";
pub(super) fn preview_text(answer: &OwnerPreviewSuccess) -> String {
    let renewal = answer
        .preview
        .last_seen
        .map_or_else(|| "unknown".into(), |time| time.unix_millis().to_string());
    let presence = answer
        .last_seen
        .map_or_else(|| "unknown".into(), |time| time.unix_millis().to_string());
    format!(
        "Holder installation: {}\nHolder peer: {}\nFence: {}\nAcknowledged: epoch {}, sequence {}\nLast renewal: {renewal} (Unix milliseconds)\nLast seen: {presence} (Unix milliseconds)\n{UNSENT_NOTICE}\n",
        answer.holder_installation.as_str(),
        answer.preview.holder,
        answer.preview.fence.value(),
        answer.preview.acked.epoch.value(),
        answer.preview.acked.sequence.value()
    )
}
fn emit<T: serde::Serialize>(answer: &T, json: bool, code: i32) -> i32 {
    let result = if json {
        serde_json::to_string(answer)
    } else {
        serde_json::to_string_pretty(answer)
    };
    match result {
        Ok(output) => match writeln!(std::io::stdout(), "{output}") {
            Ok(()) => code,
            Err(_) => 3,
        },
        Err(_) => 3,
    }
}
pub(super) fn print_answer(answer: CentralAdminAnswer, json: bool) -> i32 {
    match answer {
        CentralAdminAnswer::Copies(page) => emit(&page, json, 0),
        CentralAdminAnswer::Health(health) => emit(&health, json, 0),
        CentralAdminAnswer::Repair(answer) => emit(&answer, json, 0),
        CentralAdminAnswer::Migrate(page) => {
            let code = if page
                .results
                .iter()
                .any(|item| matches!(item.result, CopyMigrationResult::Failed { .. }))
            {
                4
            } else {
                0
            };
            emit(&page, json, code)
        }
        CentralAdminAnswer::Preview(OwnerPreviewResponse::Ready(preview)) => {
            if json {
                emit(&preview, true, 0)
            } else {
                match write!(std::io::stdout(), "{}", preview_text(&preview)) {
                    Ok(()) => 0,
                    Err(_) => 3,
                }
            }
        }
        CentralAdminAnswer::Preview(OwnerPreviewResponse::Refused(refusal)) => {
            emit(&refusal, json, 4)
        }
        CentralAdminAnswer::Transfer(answer) => {
            let code = match &answer {
                OwnerTransferAnswer::Moved { .. }
                | OwnerTransferAnswer::Granted { .. }
                | OwnerTransferAnswer::Pending { .. } => 0,
                OwnerTransferAnswer::Refused(_)
                | OwnerTransferAnswer::ConflictingRequest
                | OwnerTransferAnswer::Cancelled { .. }
                | OwnerTransferAnswer::Failed { .. } => 4,
                OwnerTransferAnswer::Unavailable { .. } => 3,
            };
            emit(&answer, json, code)
        }
        CentralAdminAnswer::Retirement(answer) => {
            let code = match &answer {
                OwnerRetirementAnswer::Retired { .. } | OwnerRetirementAnswer::Pending { .. } => 0,
                OwnerRetirementAnswer::Refused(_)
                | OwnerRetirementAnswer::ConflictingRequest
                | OwnerRetirementAnswer::NotFound => 4,
                OwnerRetirementAnswer::Unavailable { .. } => 3,
            };
            emit(&answer, json, code)
        }
        CentralAdminAnswer::Failure(failure) => {
            let code = match &failure {
                OwnerOperationFailure::InvalidPageLimit => 2,
                OwnerOperationFailure::Retired => 4,
                OwnerOperationFailure::ObjectUnavailable
                | OwnerOperationFailure::CentralCopyFull
                | OwnerOperationFailure::CopyUnavailable { .. } => 3,
            };
            emit(&failure, json, code)
        }
        CentralAdminAnswer::Unauthorized => crate::endpoint_commands::report_failure(
            "rejected",
            "Owner sign-in is required by the central admin application",
            4,
            json,
        ),
    }
}
