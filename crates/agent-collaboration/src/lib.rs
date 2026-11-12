//! Command-line and terminal interfaces to the collaboration SDK.
mod session_environment;
pub use session_environment::CliContext;
use session_environment::app_server_socket_or_default;
pub use session_environment::{RunArgumentsFailure, run_arguments};
#[cfg(feature = "quota-reset-test-harness")]
mod claude_terminal_test_harness;
mod details_client_composition;
mod session_client_composition;
#[path = "session_command_dispatch.rs"]
mod sessions;
#[cfg(feature = "quota-reset-test-harness")]
pub fn run_sessions_picker_test_harness() -> std::io::Result<()> {
    if std::env::args().nth(2).as_deref() == Some("--claude-terminal-lifecycle") {
        claude_terminal_test_harness::run()
    } else if std::env::args().nth(2).as_deref() == Some("--provider-filter-storyboard") {
        agent_session_tui::session_picker::run_sessions_picker_test_harness()
    } else {
        sessions::run_stored_model_terminal_fixture()
    }
}

/// Public executable usage generated from its argument parser.
#[must_use]
pub fn command_help() -> String {
    let help = format!(
        "{}\nSessions and providers:\n  conversation create --endpoint ID --cwd PATH --access write-restricted|workspace-write [--operation-id UUID] [--mode ID] [--model ID] [--effort LEVEL] [--fork SESSION] [--generation JSON] [--timeout-seconds 300] --json\n  conversation settings set --target ADDRESS --actor ADDRESS --setting mode|model|effort --value ID --json\n  conversation settings accept --target ADDRESS --actor ADDRESS --json\n  conversation prompt --endpoint ID --new|--session ID|--fork ID|--to ADDRESS [--cwd PATH] [--operation-id UUID] [--prompt-operation-id UUID] [--generation JSON] --text-file PATH --json\n  conversation load --target ADDRESS --cwd PATH --access write-restricted|workspace-write [--operation-id UUID] [--generation JSON] --json\n  conversation resume --target ADDRESS --cwd PATH --access write-restricted|workspace-write [--operation-id UUID] [--generation JSON] --json\n  conversation close --target ADDRESS [--operation-id UUID] [--generation JSON] --json\n  conversation cancel --target ADDRESS --target-operation-id UUID [--operation-id UUID] [--generation JSON] --json\n  conversation operation show|wait|reconcile --operation-id UUID --json\n  sessions list --endpoint ID --view stored|loaded|active --cwd PATH|--checkout PATH|--repo PATH|--any [--source interactive|subagents|all] --json\n  events listen --endpoint ID --session ID --attach [--timeout-seconds 60]\n  whoami [--json] (this agent's SessionRef from CODEX_THREAD_ID, CLAUDE_CODE_SESSION_ID or CURSOR_CONVERSATION_ID)\n  endpoints list --json [--service-directory PATH]\n  addresses list --endpoint ID --json [--cursor CURSOR]\n  session inspect --endpoint ID --session ID --json\n  session rename --endpoint ID --session ID --name TEXT --json\n  turn interrupt --endpoint ID --session ID --turn ID --json\n  native --endpoint ID\n  acp --endpoint ID\n  journal status --json\n  journal read --endpoint ID --journal-id UUID --after SEQUENCE --json\n",
        agent_session_inventory::sessions::command_help("agent-collaboration")
    );
    help.replace(
        "  events listen --endpoint ID --session ID --attach [--timeout-seconds 60]\n",
        "  events listen --endpoint ID --session ID --attach [--timeout-seconds 60]\n  events observe --endpoint ID --session ID --attach [--timeout-seconds 60] [--max-events 256] [--max-bytes 262144] [--after-sequence N --epoch N]\n  approval list|decide --help\n  question list|answer --help\n",
    )
}

/// Usage for the standalone session picker executable.
#[must_use]
pub fn session_command_help() -> String {
    agent_session_inventory::sessions::command_help("agent-sessions")
}

mod current_session_identity;
mod endpoint_commands;
mod permission_diagnostic_reporting;
pub use endpoint_commands::run_endpoint_command;
mod whoami_command;
pub use whoami_command::run_whoami_command;
mod address_book_commands;
mod approval_commands;
mod question_commands;
pub use address_book_commands::run_address_command;
pub use approval_commands::run_approval_command;
pub use question_commands::run_question_command;

mod journal_read_commands;
mod native_stdio_bridge;
pub use journal_read_commands::run_journal_command;
pub use native_stdio_bridge::run_native_command;
mod native_session_commands;
pub use native_session_commands::run_native_session_command;

mod event_commands;
pub use event_commands::run_event_command;

mod session_inventory_commands;
mod session_target_arguments;
pub use session_inventory_commands::run_session_inventory_command;

mod conversation_commands;
mod conversation_operation_commands;
mod conversation_settings_commands;
mod provider_conversation_commands;
pub use conversation_commands::run_conversation_command;

mod acp_stdio_bridge;
pub use acp_stdio_bridge::run_acp_command;

#[cfg(test)]
mod session_contract_tests;

mod automation_argument_feedback;
