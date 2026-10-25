//! Descriptive message-board commands over the public communication client.
mod board_arguments;
mod board_entity_arguments;
mod board_entity_execution;
mod board_entity_metadata_preparation;
mod board_entity_preparation;
mod board_execution;
mod board_inbox_arguments;
mod board_preparation;
mod board_receipt_arguments;
mod board_request_arguments;
mod board_search_commands;
mod board_subscription_arguments;
mod board_thread_list_preparation;
mod board_thread_subscription_preparation;
mod board_thread_wait_preparation;
mod board_value_parsing;
mod board_view_commands;
mod board_wait_execution;

use board_arguments::BoardArguments;
use std::ffi::OsString;

/// Runs a board command without opening board storage in the CLI process.
pub fn run_board_command(mut arguments: Vec<OsString>) -> i32 {
    arguments.insert(0, OsString::from("agent-collaboration"));
    let machine = arguments.iter().any(|argument| argument == "--json");
    let parsed =
        match crate::automation_argument_feedback::parse_arguments::<BoardArguments>(arguments) {
            Ok(parsed) => parsed,
            Err(code) => return code,
        };
    let (command, context) = match board_preparation::prepare(parsed.command) {
        Ok(prepared) => prepared,
        Err(message) => {
            return crate::endpoint_commands::report_failure("invalidField", &message, 2, machine);
        }
    };
    board_execution::execute(command, context)
}

#[cfg(test)]
mod claim_arguments_tests {
    use super::{
        board_arguments::BoardArguments,
        board_preparation::{self, PreparedBoardCommand},
    };
    use clap::Parser;
    #[test]
    fn claim_preserves_the_explicit_attempt_without_accepting_actor_authority() {
        let project = "019f0000-0000-7000-8000-000000009700";
        let attempt = "019f0000-0000-7000-8000-000000009701";
        let argv = [
            "agent-collaboration",
            "project",
            "claim",
            project,
            "--request-id",
            attempt,
            "--json",
        ];
        let parsed = BoardArguments::try_parse_from(argv).expect("claim command");
        let (prepared, _) = board_preparation::prepare(parsed.command).expect("claim preparation");
        let PreparedBoardCommand::ProjectClaim(request) = prepared else {
            panic!("claim prepared another command");
        };
        assert_eq!(request.project_id.as_str(), project);
        assert_eq!(request.request_id.as_str(), attempt);
        assert_eq!(
            request.protocol_version,
            message_board::ProtocolVersion::CURRENT
        );
        let mut actor = argv.to_vec();
        actor.extend(["--actor", "human:someone"]);
        assert!(BoardArguments::try_parse_from(actor).is_err());
        assert!(
            BoardArguments::try_parse_from([
                "agent-collaboration",
                "project",
                "claim",
                "bad-project",
                "--request-id",
                attempt
            ])
            .is_ok()
        );
        let parsed = BoardArguments::try_parse_from([
            "agent-collaboration",
            "project",
            "claim",
            "bad-project",
            "--request-id",
            attempt,
        ])
        .expect("raw input");
        assert!(board_preparation::prepare(parsed.command).is_err());
    }
}
