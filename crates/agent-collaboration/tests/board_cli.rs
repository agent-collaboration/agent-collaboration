use std::process::Command;

#[test]
fn inbox_fetch_help_separates_top_level_scope_from_read_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["inbox", "fetch", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for flag in [
        "--scope",
        "--workspace-id",
        "--board-id",
        "--project-id",
        "--read-mode",
    ] {
        assert!(help.contains(flag), "missing {flag}: {help}");
    }
    assert!(help.contains("independently watched threads"));
}

#[test]
fn resource_and_message_search_have_distinct_filtered_commands() {
    for arguments in [vec!["search", "--help"], vec!["post", "search", "--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let help = String::from_utf8_lossy(&output.stdout);
        for flag in [
            "--query",
            "--scope",
            "--kind",
            "--include-archived",
            "--limit",
            "--cursor",
        ] {
            assert!(help.contains(flag), "missing {flag}: {help}");
        }
    }
}

const HUMAN_ACTOR: &str = r#"{"kind":"human","humanId":"cli-test-human"}"#;

#[test]
fn thread_list_help_documents_repository_default_and_project_selector() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "list", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("current Git repository"), "{help}");
    assert!(help.contains("--repository-path"), "{help}");
    assert!(help.contains("--workspace-id"), "{help}");
}

#[test]
fn thread_list_outside_repository_shows_corrected_example() {
    let directory =
        std::env::temp_dir().join(format!("board-thread-list-outside-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "list", "--json"])
        .current_dir(&directory)
        .output()
        .unwrap();
    std::fs::remove_dir(&directory).unwrap();
    assert_eq!(output.status.code(), Some(2));
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(rendered.contains("Choose exactly one"), "{rendered}");
    assert!(
        rendered.contains("agent-collaboration thread list"),
        "{rendered}"
    );
    assert!(rendered.contains("--repository-path"), "{rendered}");
    assert!(rendered.contains("--workspace-id"), "{rendered}");
}

#[test]
fn conversation_create_help_explains_codex_and_provider_setting_rules() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["conversation", "create", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("required for Codex endpoints"), "{help}");
    assert!(
        help.contains("provider endpoints accept advertised"),
        "{help}"
    );
    assert!(help.contains("--mode <MODE>"), "{help}");
    assert!(help.contains("--model <MODEL>"), "{help}");
    assert!(help.contains("--effort <EFFORT>"), "{help}");
}

#[test]
fn thread_wait_help_exposes_subscription_filter_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "wait", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for required in [
        "--actor",
        "--max-wait",
        "--thread-id",
        "--project-id",
        "--json",
    ] {
        assert!(help.contains(required), "missing {required}: {help}");
    }
    for removed in ["--watched", "--from", "--acknowledge", "--no-acknowledge"] {
        assert!(
            !help.contains(removed),
            "unexpected legacy flag {removed}: {help}"
        );
    }
}

#[test]
fn thread_wait_filter_parsing_accepts_each_scope_and_rejects_invalid_roots() {
    let root = "018f6f67-64d2-7a21-bf9a-8f193f987001";
    let topic = "018f6f67-64d2-7a21-bf9a-8f193f987002";
    let absent_service = "/tmp/absent-thread-subscription-wait";
    for filter in [
        vec![],
        vec!["--thread-id", root],
        vec!["--project-id", topic],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(["thread", "wait", "--actor", HUMAN_ACTOR, "--max-wait", "0s"])
            .args(filter)
            .args(["--service-directory", absent_service, "--json"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(3));
    }

    for (filter, expected) in [
        (
            vec!["--thread-id", root, "--project-id", topic],
            "cannot be used with",
        ),
        (
            vec!["--thread-id", root, "--thread-id", root],
            "--thread-id values must be unique",
        ),
        (
            vec!["--thread-id", ""],
            "--thread-id must be a canonical lowercase UUIDv7",
        ),
        (vec!["--max-wait", "26m"], "at most 1500 seconds"),
    ] {
        let mut arguments = vec!["thread", "wait", "--actor", HUMAN_ACTOR];
        if filter.first().copied() != Some("--max-wait") {
            arguments.extend(["--max-wait", "1s"]);
        }
        arguments.extend(filter);
        arguments.extend(["--json"]);
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let rendered = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}

#[test]
fn participant_commands_expose_explicit_create_join_leave_and_list_choices() {
    for (arguments, required) in [
        (
            vec!["thread", "create", "--help"],
            vec![
                "--project-id",
                "--actor",
                "--role",
                "--watch",
                "--no-watch",
                "--text-file",
                "--about",
                "--json",
            ],
        ),
        (
            vec!["thread", "join", "--help"],
            vec![
                "--thread-id",
                "--actor",
                "--role",
                "--watch",
                "--no-watch",
                "--mode",
                "--when-idle",
                "watching is the default",
            ],
        ),
        (
            vec!["thread", "leave", "--help"],
            vec!["--thread-id", "--actor", "--to", "--resolve", "--json"],
        ),
        (
            vec!["thread", "participant", "list", "--help"],
            vec!["--thread-id", "--limit", "--cursor", "--json"],
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        for flag in required {
            assert!(help.contains(flag), "missing {flag}: {help}");
        }
    }
}

#[test]
fn participant_commands_refuse_omitted_semantic_choices_before_connection() {
    let root = "018f6f67-64d2-7a21-bf9a-8f193f987001";
    let topic = "018f6f67-64d2-7a21-bf9a-8f193f987002";
    for (arguments, expected) in [
        (
            vec![
                "thread",
                "create",
                "--project-id",
                topic,
                "--actor",
                HUMAN_ACTOR,
                "--text-file",
                "/tmp/participant-create-root.txt",
                "--about",
                "project:Explicit semantic choices",
                "--json",
            ],
            "--watch or --no-watch",
        ),
        (
            vec!["thread", "participant", "list", "--thread-id", root],
            "Thread Participant List requires --json",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let rendered = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(rendered.contains(expected));
    }
}

#[test]
fn thread_subscription_help_exposes_scope_policy_and_reader_commands() {
    for (arguments, required) in [
        (
            vec!["thread", "subscribe", "--help"],
            vec![
                "--thread-id",
                "--project-id",
                "--actor",
                "--mode",
                "--when-idle",
                "--quiet",
                "--cap",
                "--for",
            ],
        ),
        (
            vec!["thread", "unsubscribe", "--help"],
            vec!["--thread-id", "--project-id", "--actor"],
        ),
        (vec!["thread", "subscriptions", "--help"], vec!["--actor"]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        for flag in required {
            assert!(help.contains(flag), "missing {flag}: {help}");
        }
    }
}

#[test]
fn subscription_policy_durations_enforce_the_contract_bounds_before_connection() {
    let root = "018f6f67-64d2-7a21-bf9a-8f193f987001";
    for (options, expected) in [
        (
            vec!["--quiet", "31m"],
            "--quiet must be between 0 seconds and 30 minutes",
        ),
        (
            vec!["--quiet", "2m", "--cap", "1m"],
            "invalid subscription capSeconds: must be at least quietSeconds",
        ),
        (vec!["--cap", "61m"], "--cap must be at most 60 minutes"),
        (
            vec!["--for", "9m"],
            "invalid subscription forSeconds: must be between 600 seconds and 604800 seconds",
        ),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([
                "thread",
                "subscribe",
                "--thread-id",
                root,
                "--actor",
                HUMAN_ACTOR,
            ])
            .args(options)
            .arg("--json")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        let rendered = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}

#[test]
fn removed_listen_commands_and_join_flags_are_absent() {
    let thread_help = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "--help"])
        .output()
        .unwrap();
    assert!(thread_help.status.success());
    let thread_help = String::from_utf8_lossy(&thread_help.stdout);
    assert!(
        !thread_help.contains("listen"),
        "legacy command remains: {thread_help}"
    );

    let join_help = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "join", "--help"])
        .output()
        .unwrap();
    assert!(join_help.status.success());
    let join_help = String::from_utf8_lossy(&join_help.stdout);
    for removed_flag in ["--listen", "--acknowledge", "--no-acknowledge"] {
        assert!(
            !join_help.contains(removed_flag),
            "legacy join flag {removed_flag} remains: {join_help}"
        );
    }
}

#[test]
fn thread_watch_accepts_exactly_one_thread_or_topic_target() {
    let topic = "018f6f67-64d2-7a21-bf9a-8f193f987002";
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "thread",
            "watch",
            "--project-id",
            topic,
            "--actor",
            HUMAN_ACTOR,
            "--service-directory",
            "/tmp/absent-topic-watch",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));

    let invalid = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["thread", "watch", "--actor", HUMAN_ACTOR, "--json"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
}

#[test]
fn board_help_lists_every_descriptive_command_family() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["board", "--help"])
        .output()
        .unwrap_or_else(|error| panic!("board help should execute: {error}"));

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let help = String::from_utf8_lossy(&output.stdout);
    for command in ["create", "update", "show", "list", "archive"] {
        assert!(
            help.contains(command),
            "board help omitted {command}: {help}"
        );
    }
    for old_child in ["project", "topic", "message"] {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(old_child)),
            "old child {old_child}: {help}"
        );
    }
}

#[test]
fn message_list_help_exposes_scope_selection_and_paging_as_distinct_inputs() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["post", "list", "--help"])
        .output()
        .unwrap_or_else(|error| panic!("message list help should execute: {error}"));

    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    for flag in [
        "--scope",
        "--selection",
        "--after",
        "--from",
        "--to",
        "--limit",
        "--cursor",
    ] {
        assert!(
            help.contains(flag),
            "message list help omitted {flag}: {help}"
        );
    }
    assert!(help.contains("all-workspaces"));
    assert!(help.contains("after-position"));
}

#[test]
fn invalid_actor_is_rejected_before_service_discovery_without_echoing_identity_payload() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "workspace",
            "create",
            "--name",
            "Coordination",
            "--actor",
            r#"{"kind":"session","session":{"secret":"must-not-leak"}}"#,
            "--service-directory",
            "/tmp/absent-board-cli-service",
            "--json",
        ])
        .output()
        .unwrap_or_else(|error| panic!("invalid actor command should execute: {error}"));

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("--actor must be valid typed Identity JSON"));
    assert!(!result.contains("must-not-leak"));
    assert!(!result.contains("boardUnavailable"));
}

#[test]
fn project_target_post_is_rejected_before_service_discovery() {
    let id = "018f6f67-64d2-7a21-bf9a-8f193f987001";
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "post",
            "post",
            "--placement",
            "project",
            "--thread-id",
            id,
            "--text",
            "hello",
            "--actor",
            HUMAN_ACTOR,
            "--json",
        ])
        .output()
        .unwrap_or_else(|error| panic!("invalid placement command should execute: {error}"));

    assert_eq!(output.status.code(), Some(2));
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("invalid value 'project'"), "{result}");
    assert!(result.contains("possible values: thread"), "{result}");
    assert!(!result.contains("boardUnavailable"));
}

#[test]
fn range_validation_rejects_reversed_activity_bounds_before_connecting() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "post",
            "list",
            "--scope",
            "all-workspaces",
            "--selection",
            "range",
            "--from",
            r#"{"kind":"project","cursor":{"epoch":1,"position":9}}"#,
            "--to",
            r#"{"kind":"project","cursor":{"epoch":1,"position":4}}"#,
            "--json",
        ])
        .output()
        .unwrap_or_else(|error| panic!("invalid range command should execute: {error}"));

    assert_eq!(output.status.code(), Some(2));
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("range start must not exceed range end"));
}

#[test]
fn message_list_omits_selection_to_use_latest_default() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "post",
            "list",
            "--scope",
            "all-workspaces",
            "--service-directory",
            "/tmp/absent-board-cli-service",
            "--json",
        ])
        .output()
        .unwrap_or_else(|error| panic!("default latest command should parse: {error}"));

    assert_eq!(output.status.code(), Some(3));
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("boardUnavailable"));
    assert!(!result.contains("--selection"));
}

#[test]
fn creation_and_post_help_expose_exact_utf8_and_reference_bounds() {
    let project_help = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["workspace", "create", "--help"])
        .output()
        .unwrap();
    assert!(project_help.status.success());
    let project_help = String::from_utf8_lossy(&project_help.stdout);
    assert!(project_help.contains("1 to 256 UTF-8 bytes"));
    assert!(project_help.contains("0 to 16384 UTF-8 bytes"));

    let message_help = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["post", "post", "--help"])
        .output()
        .unwrap();
    assert!(message_help.status.success());
    let message_help = String::from_utf8_lossy(&message_help.stdout);
    assert!(message_help.contains("1 to 65536 UTF-8 bytes"));
    assert!(message_help.contains("at most 64 distinct references total"));
}

#[test]
fn bounded_text_errors_name_exact_limits_without_echoing_input() {
    let secret_value = format!("secret-value-{}", "x".repeat(256));
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "workspace",
            "create",
            "--name",
            &secret_value,
            "--actor",
            HUMAN_ACTOR,
            "--json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let result = String::from_utf8_lossy(&output.stdout);
    assert!(result.contains("--name must contain 1 to 256 UTF-8 bytes after trimming"));
    assert!(!result.contains(&secret_value));
}

#[test]
fn mutation_discovery_failure_is_known_unavailable_before_transmission() {
    let project_id = "018f6f67-64d2-7a21-bf9a-8f193f987090";
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "workspace",
            "create",
            "--workspace-id",
            project_id,
            "--name",
            "No service",
            "--actor",
            HUMAN_ACTOR,
            "--service-directory",
            "/tmp/absent-board-pretransmission-service",
            "--json",
        ])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(3));
    let response = String::from_utf8_lossy(&output.stdout);
    assert!(response.contains("boardUnavailable"));
    assert!(!response.contains("outcomeUnknown"));
    assert!(!response.contains(project_id));
}

#[test]
fn combined_board_command_argument_reports_shell_tokenization_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .arg("board project list")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("board command was passed as one argument"));
    assert!(error.contains("Pass each word as a separate argument"));
    assert!(!error.contains("debug"));
    assert!(!error.contains("socket"));
}

#[test]
fn combined_board_command_argument_preserves_requested_json_error_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .arg("board project list --json")
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        error
            .pointer("/error/kind")
            .and_then(serde_json::Value::as_str),
        Some("invalidUsage")
    );
    assert_eq!(
        error
            .pointer("/error/nextAction")
            .and_then(serde_json::Value::as_str),
        Some("correctRequest")
    );
}

#[test]
fn combined_first_argument_is_rejected_even_with_separate_trailing_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args(["board project list", "--help"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("board command was passed as one argument")
    );

    let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
        .args([
            "board project list",
            "--service-directory",
            "/tmp/absent-combined-command",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stderr.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        error
            .pointer("/error/kind")
            .and_then(serde_json::Value::as_str),
        Some("invalidUsage")
    );
}

#[test]
fn board_model_command_groups_have_new_names_and_keep_no_level_aliases() {
    for group in [
        "workspace",
        "board",
        "project",
        "workstream",
        "task",
        "subtask",
        "work-trail",
        "thread",
        "role",
        "move",
        "fact",
        "feed",
        "adr",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args([group, "--help"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{group}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for old in [["board", "project", "--help"], ["board", "topic", "--help"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(old)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "retired level alias still accepted: {old:?}"
        );
    }
}

#[test]
fn board_kind_and_thread_domain_targets_are_explicit_cli_inputs() {
    for (arguments, flags) in [
        (vec!["board", "create", "--help"], vec!["--kind"]),
        (
            vec!["thread", "create", "--help"],
            vec!["--about", "--addressee"],
        ),
        (vec!["post", "post", "--help"], vec!["--addressee"]),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .output()
            .unwrap();
        assert!(output.status.success());
        let help = String::from_utf8_lossy(&output.stdout);
        for flag in flags {
            assert!(help.contains(flag), "missing {flag}: {help}");
        }
    }
}

#[test]
fn thread_commands_parse_project_qualified_targets_before_connecting() {
    let qualified = "018f6f67-64d2-7a21-bf9a-8f193f987002/018f6f67-64d2-7a21-bf9a-8f193f987001";
    for arguments in [
        vec!["thread", "show", "--thread-id", qualified],
        vec!["thread", "participant", "list", "--thread-id", qualified],
        vec![
            "thread",
            "watch",
            "--thread-id",
            qualified,
            "--actor",
            HUMAN_ACTOR,
        ],
        vec![
            "thread",
            "subscribe",
            "--thread-id",
            qualified,
            "--actor",
            HUMAN_ACTOR,
        ],
        vec![
            "thread",
            "wait",
            "--thread-id",
            qualified,
            "--actor",
            HUMAN_ACTOR,
            "--max-wait",
            "0s",
        ],
        vec![
            "post",
            "list",
            "--scope",
            "thread",
            "--thread-id",
            qualified,
        ],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(&arguments)
            .args([
                "--service-directory",
                "/tmp/absent-thread-reference-proof",
                "--json",
            ])
            .output()
            .expect("CLI executes");
        assert_eq!(
            output.status.code(),
            Some(3),
            "{arguments:?}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
