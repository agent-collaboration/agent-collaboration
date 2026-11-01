use super::{
    MessageSendArguments, ReplyArguments, operation_failure_exit, operation_failure_line,
    report_message_failure, validate_push_reference,
};
use crate::failure_line::{
    render_failure_line, render_held_line, render_rejection_line, render_unknown_line,
};
use crate::message_input_arguments::prepare_message_send;
use collaboration_client::protocol::{
    DeliveryOutcome, MessageReplyResult, MessageSendResult, MessageText, PushDeliveryState,
    QueuedMessageSend,
};
use collaboration_client::{
    ClientError, CollaborationClient, MessageReplyError, MessageReplyRequest, MessageSendError,
    MessageSendRequest, OperationEffect,
};
use serde_json::json;
use std::io::{self, Read, Write};

pub(super) fn run_message_send(args: MessageSendArguments) -> i32 {
    let machine = args.json;
    let item_id = match args
        .item_id
        .clone()
        .map(collaboration_client::protocol::MailItemId::try_from)
        .transpose()
    {
        Ok(id) => id.unwrap_or_else(collaboration_client::protocol::MailItemId::generate),
        Err(_) => {
            return report_message_failure(
                "invalidShape",
                "--item-id must be a canonical UUIDv7",
                2,
                machine,
            );
        }
    };
    let prepared = prepare_message_send(&args);
    let (directory, prepared) = match prepared {
        Ok(value) => value,
        Err(message) => {
            return report_message_failure("invalidField", &message, 2, machine);
        }
    };
    if writeln!(io::stderr(), "itemId: {}", item_id.as_str()).is_err() {
        return 5;
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(value) => value,
        Err(_) => {
            return report_message_failure("unavailable", "Client runtime unavailable", 3, machine);
        }
    };
    let outcome = runtime.block_on(async {
        let client = CollaborationClient::connect(
            &directory,
            "agent-collaboration",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|error| MessageSendError::Preparation(Box::new(error)))?;
        let saved = prepared
            .resolve_direct_message(&client.identity().service_id)
            .map_err(|_| {
                MessageSendError::Preparation(Box::new(ClientError::InvalidRequest(
                    "invalid session target",
                )))
            })?;
        let request = MessageSendRequest {
            item_id: Some(item_id),
            target: saved.target,
            message: saved
                .content
                .try_into()
                .map_err(|error| MessageSendError::Preparation(Box::new(error)))?,
            delivery: saved.delivery,
            generation_guard: saved.generation_guard,
        };
        client.send_message(request).await
    });
    report(outcome, machine)
}

pub(super) fn run_message_reply(args: ReplyArguments) -> i32 {
    let machine = args.json;
    let reference = args.reference.clone();
    if let Err(message) = validate_push_reference(&reference) {
        return report_message_failure("invalidField", &message, 2, machine);
    }
    let reply_text = match read_reply_text(&args) {
        Ok(text) => text,
        Err(message) => {
            return report_message_failure("invalidField", &message, 2, machine);
        }
    };
    let harness_identity =
        match collaboration_client::current_session_identity::read_harness_session_identity() {
            Ok(identity) => identity,
            Err(error) => {
                return report_message_failure(
                    "currentSessionUnavailable",
                    &format!("{error}; run agent-collaboration whoami --json"),
                    2,
                    machine,
                );
            }
        };
    let directory = match crate::endpoint_commands::resolve_directory(args.service_directory) {
        Ok(directory) => directory,
        Err(message) => {
            return report_message_failure("invalidField", &message, 2, machine);
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return report_message_failure("unavailable", "Client runtime unavailable", 3, machine);
        }
    };
    let outcome = runtime.block_on(async {
        let client = CollaborationClient::connect(
            &directory,
            "agent-collaboration",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|error| MessageReplyError::Preparation(Box::new(error)))?;
        let caller = harness_identity
            .session_ref(&client.identity().service_id)
            .map_err(|_| {
                MessageReplyError::Preparation(Box::new(ClientError::InvalidRequest(
                    "invalid caller session identity",
                )))
            })?;
        client
            .reply_to_push(MessageReplyRequest {
                caller,
                reference,
                text: reply_text,
            })
            .await
    });
    report_reply(outcome, machine)
}

fn read_reply_text(args: &ReplyArguments) -> Result<MessageText, String> {
    let text = if let Some(text) = &args.text {
        text.clone()
    } else {
        let path = args
            .text_file
            .as_ref()
            .ok_or_else(|| "Message content required".to_owned())?;
        let mut reader: Box<dyn Read> = if path.as_os_str() == "-" {
            Box::new(io::stdin())
        } else {
            Box::new(std::fs::File::open(path).map_err(|_| "Message file unavailable")?)
        };
        let mut text = String::new();
        reader
            .by_ref()
            .take((collaboration_client::protocol::MAX_CONTROL_FRAME_BYTES + 1) as u64)
            .read_to_string(&mut text)
            .map_err(|_| "Cannot read UTF-8 message")?;
        text
    };
    text.try_into()
        .map_err(|_| "Invalid or oversized message text".to_owned())
}

fn report_reply(result: Result<MessageReplyResult, MessageReplyError>, machine: bool) -> i32 {
    let (record, exit_code, confirmation, failure_line, write_failure_code) = match result {
        Ok(MessageSendResult::Queued { queued }) => queued_report(queued),
        Ok(MessageSendResult::Local(reply)) => {
            let exit_code = receipt_exit_status(reply.delivery_state);
            let failure_line = receipt_failure_line(
                reply.delivery_state,
                &reply.target_identity,
                &reply.link,
                &reply.receipt.outcome,
            );
            let confirmation = failure_line
                .is_none()
                .then(|| reply_confirmation_line(&reply));
            (
                crate::endpoint_commands::result_envelope(serde_json::json!(reply)),
                exit_code,
                confirmation,
                failure_line,
                5,
            )
        }
        Err(error) => {
            let (failure, caller) = error.into_operation_failure_and_caller();
            let exit_code = operation_failure_exit(&failure);
            let failure_line = operation_failure_line(&failure, None);
            (
                serde_json::json!({"kind":"error","caller":caller,"error":failure}),
                exit_code,
                None,
                Some(failure_line),
                if exit_code == 5 { 5 } else { 3 },
            )
        }
    };
    let written = if machine {
        writeln!(io::stdout(), "{record}")
    } else if let Some(failure_line) = failure_line {
        writeln!(io::stderr(), "{failure_line}")
    } else if let Some(confirmation) = confirmation {
        writeln!(io::stdout(), "{confirmation}")
    } else {
        writeln!(
            io::stdout(),
            "{}",
            serde_json::to_string_pretty(&record)
                .unwrap_or_else(|_| "Output unavailable".to_owned())
        )
    };
    if written.is_err() {
        write_failure_code
    } else {
        exit_code
    }
}

fn queued_report(
    queued: QueuedMessageSend,
) -> (serde_json::Value, i32, Option<String>, Option<String>, i32) {
    let confirmation = format!(
        "queued item {} to {} {}",
        queued.item.as_str(),
        queued.target_identity,
        serde_json::to_string(&queued.target)
            .unwrap_or_else(|_| "SessionRef unavailable".to_owned())
    );
    let result: MessageSendResult = MessageSendResult::Queued { queued };
    (
        crate::endpoint_commands::result_envelope(json!(result)),
        0,
        Some(confirmation),
        None,
        5,
    )
}

fn reply_confirmation_line(
    reply: &collaboration_client::protocol::SessionMessageReplyResult,
) -> String {
    format!(
        "replied to {} {} (push {} {}): {}",
        reply.target_identity,
        session_ref_text(reply),
        reply.push_id.as_str(),
        reply.link,
        delivery_outcome_label(&reply.receipt.outcome)
    )
}

fn session_ref_text(reply: &collaboration_client::protocol::SessionMessageReplyResult) -> String {
    serde_json::to_string(&reply.target).unwrap_or_else(|_| "SessionRef unavailable".to_owned())
}

fn report(result: Result<MessageSendResult, MessageSendError>, machine: bool) -> i32 {
    if let Err(MessageSendError::Preparation(error)) = &result
        && let Some(code) = crate::permission_diagnostic_reporting::report_actionable_client_error(
            error,
            crate::permission_diagnostic_reporting::PermissionDiagnosticRendering::Command,
            machine,
        )
    {
        return code;
    }
    let (record, code, confirmation, failure_line, write_failure_code) = match result {
        Ok(MessageSendResult::Queued { queued }) => queued_report(queued),
        Ok(MessageSendResult::Local(push)) => {
            let exit = receipt_exit_status(push.delivery_state);
            let failure_line = receipt_failure_line(
                push.delivery_state,
                &push.target_identity,
                &push.link,
                &push.receipt.outcome,
            );
            let confirmation = failure_line
                .is_none()
                .then(|| send_confirmation_line(&push));
            let record = crate::endpoint_commands::result_envelope(json!(push));
            (record, exit, confirmation, failure_line, 5)
        }
        Err(error) => {
            let (failure, target) = error.into_operation_failure_and_target();
            let exit = operation_failure_exit(&failure);
            let human_failure = operation_failure_line(&failure, target.as_ref());
            let write_failure = if failure.effect == OperationEffect::Unknown {
                5
            } else {
                3
            };
            (
                json!({"kind":"error","target":target,"error":failure}),
                exit,
                None,
                Some(human_failure),
                write_failure,
            )
        }
    };
    let written = if machine {
        writeln!(io::stdout(), "{record}")
    } else if let Some(failure_line) = failure_line {
        writeln!(io::stderr(), "{failure_line}")
    } else if let Some(confirmation) = confirmation {
        writeln!(io::stdout(), "{confirmation}")
    } else {
        writeln!(
            io::stdout(),
            "{}",
            serde_json::to_string_pretty(&record)
                .unwrap_or_else(|_| "Output unavailable".to_owned())
        )
    };
    if written.is_err() {
        write_failure_code
    } else {
        code
    }
}

fn send_confirmation_line(push: &collaboration_client::protocol::PushMessageSendResult) -> String {
    format!(
        "push {} to {} {}: {}; {}",
        push.push_id.as_str(),
        push.target_identity,
        serde_json::to_string(&push.target).unwrap_or_else(|_| "SessionRef unavailable".to_owned()),
        delivery_outcome_label(&push.receipt.outcome),
        push.link
    )
}

fn delivery_outcome_label(outcome: &DeliveryOutcome) -> String {
    match outcome {
        DeliveryOutcome::Started => "started".to_owned(),
        DeliveryOutcome::Steered => "steered".to_owned(),
        DeliveryOutcome::StartedOrSteered => "started or steered".to_owned(),
        DeliveryOutcome::Queued => "queued".to_owned(),
        DeliveryOutcome::PeerMessageWritten => "peer message written".to_owned(),
        DeliveryOutcome::NotSubmitted { retryable, reason } => format!(
            "not submitted{}: {reason}",
            if *retryable { " (retryable)" } else { "" }
        ),
        DeliveryOutcome::Rejected(_) => "rejected".to_owned(),
        DeliveryOutcome::Unknown => "outcome unknown".to_owned(),
    }
}

fn receipt_exit_status(delivery_state: PushDeliveryState) -> i32 {
    match delivery_state {
        PushDeliveryState::Delivered | PushDeliveryState::Held => 0,
        PushDeliveryState::Rejected => 4,
        PushDeliveryState::OutcomeUnknown
        | PushDeliveryState::Pending
        | PushDeliveryState::Attempted => 5,
    }
}

fn receipt_failure_line(
    delivery_state: PushDeliveryState,
    target: &str,
    push_link: &str,
    outcome: &DeliveryOutcome,
) -> Option<String> {
    match delivery_state {
        PushDeliveryState::Held => Some(render_held_line(push_link, target)),
        PushDeliveryState::Rejected => match outcome {
            DeliveryOutcome::Rejected(rejection) => {
                Some(render_rejection_line(rejection, target, push_link))
            }
            DeliveryOutcome::NotSubmitted { reason, .. } => Some(render_failure_line(
                &format!("Delivery to {target} was rejected: {reason}"),
                "correct the message or target and try again",
            )),
            _ => Some(render_failure_line(
                &format!("Delivery to {target} was rejected"),
                "run agent-collaboration show <link> to inspect the stored outcome",
            )),
        },
        PushDeliveryState::OutcomeUnknown
        | PushDeliveryState::Pending
        | PushDeliveryState::Attempted => Some(render_unknown_line(target, push_link)),
        PushDeliveryState::Delivered => None,
    }
}

#[cfg(test)]
#[path = "message_reply_argument_tests.rs"]
mod reply_argument_tests;
#[cfg(test)]
#[path = "message_send_result_tests.rs"]
mod tests;
