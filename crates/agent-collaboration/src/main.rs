fn main() {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() == 1
        && arguments
            .first()
            .is_some_and(|arg| arg == "--version" || arg == "-V")
    {
        println!("agent-collaboration {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if arguments.len() == 1
        && arguments
            .first()
            .is_some_and(|arg| arg == "--help" || arg == "-h")
    {
        print!("{}", agent_collaboration::command_help());
        return;
    }
    if arguments.first().is_some_and(|arg| arg == "conversation") {
        std::process::exit(agent_collaboration::run_conversation_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "approval") {
        std::process::exit(agent_collaboration::run_approval_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "question") {
        std::process::exit(agent_collaboration::run_question_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "sessions") {
        std::process::exit(agent_collaboration::run_session_inventory_command(
            arguments,
        ));
    }
    if arguments.first().is_some_and(|arg| arg == "events") {
        std::process::exit(agent_collaboration::run_event_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "native") {
        std::process::exit(agent_collaboration::run_native_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "acp") {
        std::process::exit(agent_collaboration::run_acp_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "whoami") {
        std::process::exit(agent_collaboration::run_whoami_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "endpoints") {
        std::process::exit(agent_collaboration::run_endpoint_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "addresses") {
        std::process::exit(agent_collaboration::run_address_command(arguments));
    }
    if arguments.first().is_some_and(|arg| arg == "journal") {
        std::process::exit(agent_collaboration::run_journal_command(arguments));
    }
    if arguments
        .first()
        .is_some_and(|arg| arg == "session" || arg == "turn")
    {
        std::process::exit(agent_collaboration::run_native_session_command(arguments));
    }
    if let Err(error) = agent_collaboration::run_arguments(arguments) {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
