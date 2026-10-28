//! Print the stable control identity before dispatch; never retry an uncertain call.
use super::{
    MessageCancelArguments, OperationEffect, Write, io, operation_failure_exit,
    operation_failure_line, report_message_failure,
};
use collaboration_client::protocol::{MailItemId, MessageCancelRequest, MessageCancelSender};
use collaboration_client::{ClientError, CollaborationClient, operation_failure_from_client_error};
use serde_json::json;

pub(super) fn run(args: MessageCancelArguments) -> i32 {
    let target = match MailItemId::try_from(args.item) {
        Ok(id) => id,
        Err(_) => {
            return report_message_failure(
                "invalidShape",
                "--item must be a canonical UUIDv7",
                2,
                args.json,
            );
        }
    };
    let id = match args.cancel_id.map(MailItemId::try_from).transpose() {
        Ok(id) => id.unwrap_or_else(MailItemId::generate),
        Err(_) => {
            return report_message_failure(
                "invalidShape",
                "--cancel-id must be a canonical UUIDv7",
                2,
                args.json,
            );
        }
    };
    let directory = match crate::endpoint_commands::resolve_directory(args.service_directory) {
        Ok(directory) => directory,
        Err(error) => return report_message_failure("invalidField", &error, 2, args.json),
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return report_message_failure(
                "unavailable",
                "Client runtime unavailable",
                3,
                args.json,
            );
        }
    };
    if writeln!(io::stderr(), "cancelId: {}", id.as_str()).is_err() {
        return 5;
    }
    let outcome = runtime.block_on(async {
        let client = CollaborationClient::connect(
            &directory,
            "agent-collaboration",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(|error| operation_failure_from_client_error(error, OperationEffect::None))?;
        let sender = if args.human_user {
            MessageCancelSender::HumanUser
        } else {
            let sender = match args.sender {
                Some(sender) => serde_json::from_str(&sender).map_err(|_| {
                    operation_failure_from_client_error(
                        ClientError::InvalidRequest("--from must be a SessionRef"),
                        OperationEffect::None,
                    )
                })?,
                None => crate::current_session_identity::read_harness_session_identity()
                    .map_err(|_| {
                        operation_failure_from_client_error(
                            ClientError::InvalidRequest(
                                "current session identity unavailable; run whoami --json",
                            ),
                            OperationEffect::None,
                        )
                    })?
                    .session_ref(&client.identity().service_id)
                    .map_err(|_| {
                        operation_failure_from_client_error(
                            ClientError::InvalidRequest("invalid sender identity"),
                            OperationEffect::None,
                        )
                    })?,
            };
            MessageCancelSender::Agent { sender }
        };
        client
            .message_cancel(MessageCancelRequest {
                cancel_id: Some(id),
                target,
                sender,
            })
            .await
            .map_err(|error| operation_failure_from_client_error(error, OperationEffect::Unknown))
    });
    match outcome {
        Ok(result) => {
            let value = crate::endpoint_commands::result_envelope(json!(result));
            let printed = if args.json {
                writeln!(io::stdout(), "{value}")
            } else {
                writeln!(io::stdout(), "{}", result.cancel.as_str())
            };
            if printed.is_ok() { 0 } else { 5 }
        }
        Err(failure) => {
            let exit = operation_failure_exit(&failure);
            let line = if failure.effect == OperationEffect::Unknown {
                format!(
                    "Cancellation was not confirmed: {}; inspect the cancel ID before retrying",
                    failure.message
                )
            } else {
                operation_failure_line(&failure, None)
            };
            let record = json!({"kind":"error","error":failure});
            let printed = if args.json {
                writeln!(io::stdout(), "{record}")
            } else {
                writeln!(io::stderr(), "{line}")
            };
            if printed.is_ok() { exit } else { 5 }
        }
    }
}
