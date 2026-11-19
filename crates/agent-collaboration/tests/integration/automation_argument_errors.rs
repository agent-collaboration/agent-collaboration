//! Argument rejection is machine-readable before service discovery or native dispatch.

/// The local wake and schedule command groups this build no longer carries, each with a
/// command it once accepted.
const RETIRED_GROUP_COMMANDS: [[&str; 2]; 8] = [
    ["wake", "send"],
    ["schedule", "create"],
    ["instruction", "show"],
    ["run", "show"],
    ["automation", "status"],
    ["delivery", "list"],
    ["revision", "list"],
    ["operation", "show"],
];

#[test]
fn retired_command_groups_answer_not_available_in_json_and_never_reach_the_picker() {
    for [group, leaf] in RETIRED_GROUP_COMMANDS {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([group, leaf, "--json"])
            .output()
            .expect("CLI executes");
        assert_eq!(output.status.code(), Some(2), "{group} {leaf}");
        // The session picker never answers JSON; this one decodes or the command fell through.
        let value: serde_json::Value =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
                panic!(
                    "{group} {leaf} must return JSON: {error}; stderr={}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        assert_eq!(value["kind"], "error", "{group} {leaf}: {value}");
        assert_eq!(value["error"]["kind"], "invalidUsage", "{group} {leaf}");
        assert_eq!(value["error"]["stage"], "validation", "{group} {leaf}");
        assert_eq!(
            value["error"]["message"], "not available in this build",
            "{group} {leaf}"
        );
        assert_eq!(
            value["error"]["nextAction"], "correctRequest",
            "{group} {leaf}"
        );
    }
}

#[test]
fn retired_command_groups_and_their_help_refuse_in_plain_output() {
    for [group, leaf] in RETIRED_GROUP_COMMANDS {
        for arguments in [[group, leaf], [group, "--help"]] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
                .args(arguments)
                .output()
                .expect("CLI executes");
            assert_eq!(output.status.code(), Some(2), "{arguments:?}");
            assert!(output.stdout.is_empty(), "{arguments:?} printed usage");
            assert_eq!(
                String::from_utf8_lossy(&output.stderr),
                format!("agent-collaboration {group}: not available in this build\n"),
                "{arguments:?}"
            );
        }
    }
}

#[test]
fn top_level_help_no_longer_lists_retired_command_groups() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .arg("--help")
        .output()
        .expect("CLI executes");
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for [group, _] in RETIRED_GROUP_COMMANDS {
        assert!(
            !help
                .lines()
                .any(|line| line.starts_with(&format!("  {group} "))),
            "help still lists {group}: {help}"
        );
    }
    assert!(help.contains("  conversation operation show|wait|reconcile"));
}

#[test]
fn finite_control_groups_keep_json_for_missing_required_arguments() {
    let cases: &[&[&str]] = &[
        &["endpoints", "invalid-action", "--json"],
        &["addresses", "list", "--json"],
        &["journal", "read", "--json"],
        &["sessions", "list", "--json"],
        &["session", "inspect", "--json"],
        &["conversation", "prompt", "--json"],
        &["board", "project", "show", "--json"],
        &["message", "send", "--json"],
    ];

    for arguments in cases {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(*arguments)
            .output()
            .unwrap_or_else(|error| panic!("command {arguments:?}: {error}"));
        assert_eq!(output.status.code(), Some(2), "command {arguments:?}");
        let record: serde_json::Value = serde_json::from_slice(&output.stdout)
            .unwrap_or_else(|error| panic!("JSON for {arguments:?}: {error}"));
        assert_eq!(record["error"]["kind"], "invalidField");
        assert_eq!(record["error"]["stage"], "validation");
        assert_eq!(record["error"]["nextAction"], "correctRequest");
    }
}

#[test]
fn resume_fork_and_new_accept_absent_model_or_effort() {
    // Arrange: resume and fork omit the choices; --new omits them too.
    let resume = [
        "conversation",
        "prompt",
        "--endpoint",
        "codex-local",
        "--cwd",
        "/tmp",
        "--session",
        "01a0a9aa-0393-7a30-aeca-c7c77d679774",
        "--text",
        "hello",
        "--json",
    ];
    let fresh = [
        "conversation",
        "prompt",
        "--endpoint",
        "codex-local",
        "--cwd",
        "/tmp",
        "--new",
        "--model",
        "gpt-5.6-sol",
        "--access",
        "workspace-write",
        "--text",
        "hello",
        "--json",
    ];

    let forked = [
        "conversation",
        "prompt",
        "--endpoint",
        "codex-local",
        "--cwd",
        "/tmp",
        "--fork",
        "01a0a9aa-0393-7a30-aeca-c7c77d679774",
        "--access",
        "workspace-write",
        "--text",
        "hello",
        "--json",
    ];

    // A test-owned, empty service directory: the requests never reach a
    // running Router, so nothing is prompted or forked anywhere.
    let service = tempfile::tempdir().expect("service directory");
    let run = |arguments: &[&str]| {
        std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .arg("--service-directory")
            .arg(service.path())
            .output()
            .expect("CLI executes")
    };

    // Act.
    let resumed = run(&resume);
    let forked_output = run(&forked);
    let created = run(&fresh);

    // Assert: all three requests pass optional model and effort validation.
    let record: serde_json::Value =
        serde_json::from_slice(&resumed.stdout).expect("JSON conversation record");
    assert_ne!(
        record["error"]["stage"], "validation",
        "resume must not require --effort: {record}"
    );
    let fork_record: serde_json::Value =
        serde_json::from_slice(&forked_output.stdout).expect("JSON conversation record");
    assert_ne!(
        fork_record["error"]["stage"], "validation",
        "fork inherits the source thread's model and effort: {fork_record}"
    );
    assert_eq!(created.status.code(), Some(3));
    let records = serde_json::Deserializer::from_slice(&created.stdout)
        .into_iter::<serde_json::Value>()
        .collect::<Result<Vec<_>, _>>()
        .expect("create start and failure records");
    assert_eq!(records[0]["kind"], "conversationOperationStarted");
    assert!(records[0]["operationId"].as_str().is_some());
    let failure = records.last().expect("failure record");
    assert_ne!(failure["error"]["stage"], "validation", "{failure}");
}

#[test]
fn conversation_invalid_choices_are_rejected_before_service_discovery() {
    let cases = [
        (
            vec![
                "conversation",
                "prompt",
                "--endpoint",
                "codex-local",
                "--cwd",
                "/tmp",
                "--session",
                "01a0a9aa-0393-7a30-aeca-c7c77d679774",
                "--model",
                "gpt-5.6-sol",
                "--effort",
                "medium",
                "--text",
                "hello",
                "--json",
            ],
            "model is fixed for a thread; fork to change it",
        ),
        (
            vec![
                "conversation",
                "prompt",
                "--endpoint",
                "codex-local",
                "--cwd",
                "/tmp",
                "--new",
                "--model",
                "gpt-5.6-sol",
                "--effort",
                "medium",
                "--text",
                "hello",
                "--json",
            ],
            "--access",
        ),
        (
            vec![
                "conversation",
                "prompt",
                "--endpoint",
                "codex-local",
                "--cwd",
                "/tmp",
                "--new",
                "--model",
                "gpt-5.6-sol",
                "--effort",
                "medium",
                "--access",
                "full-access",
                "--text",
                "hello",
                "--json",
            ],
            "invalid value 'full-access'",
        ),
        (
            vec![
                "conversation",
                "prompt",
                "--endpoint",
                "codex-local",
                "--cwd",
                "/tmp",
                "--session",
                "01a0a9aa-0393-7a30-aeca-c7c77d679774",
                "--effort",
                "medium",
                "--access",
                "write-restricted",
                "--text",
                "hello",
                "--json",
            ],
            "access is fixed for a thread",
        ),
    ];
    for (arguments, expected) in cases {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .expect("CLI executes");
        assert_eq!(output.status.code(), Some(2));
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error");
        assert!(
            record["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains(expected)),
            "{record}"
        );
    }
}

#[test]
fn session_discovery_and_rename_validate_required_choices_before_connecting() {
    let cases: &[(&[&str], &str)] = &[
        (
            &[
                "sessions",
                "list",
                "--endpoint",
                "codex-local",
                "--view",
                "stored",
                "--source",
                "all",
                "--json",
            ],
            "--cwd",
        ),
        (
            &[
                "sessions",
                "list",
                "--endpoint",
                "codex-local",
                "--view",
                "stored",
                "--any",
                "--cwd",
                "/tmp",
                "--source",
                "all",
                "--json",
            ],
            "exactly one",
        ),
        (
            &[
                "session",
                "rename",
                "--endpoint",
                "codex-local",
                "--session",
                "01a0a9aa-0393-7a30-aeca-c7c77d679774",
                "--name",
                "",
                "--json",
            ],
            "--name",
        ),
    ];
    for (arguments, expected) in cases {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(*arguments)
            .output()
            .expect("CLI executes");
        assert_eq!(output.status.code(), Some(2), "{arguments:?}");
        let record: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON error");
        assert!(
            record["error"]["message"]
                .as_str()
                .is_some_and(|message| message.contains(expected)),
            "{record}"
        );
    }
}
