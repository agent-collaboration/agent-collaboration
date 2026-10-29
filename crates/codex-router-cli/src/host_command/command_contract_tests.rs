//! Existing Host command and root validation contracts.

use super::*;

#[test]
fn parsed_host_mcp_bind_resolves_the_effective_launch_input() {
    let cases = [
        (
            "normal omitted bind",
            Vec::new(),
            false,
            SocketAddr::from(([127, 0, 0, 1], 19742)),
        ),
        (
            "isolated debug omitted bind",
            Vec::new(),
            true,
            SocketAddr::from(([127, 0, 0, 1], 18788)),
        ),
        (
            "explicit ephemeral bind",
            vec![OsString::from("--mcp-bind"), OsString::from("127.0.0.1:0")],
            false,
            SocketAddr::from(([127, 0, 0, 1], 0)),
        ),
        (
            "explicit nondefault bind",
            vec![
                OsString::from("--mcp-bind"),
                OsString::from("127.0.0.1:19000"),
            ],
            false,
            SocketAddr::from(([127, 0, 0, 1], 19000)),
        ),
    ];

    for (case, arguments, isolated_debug, expected_bind) in cases {
        let cli_arguments = std::iter::once(OsString::from("codex-router"))
            .chain(std::iter::once(OsString::from("host")))
            .chain(arguments);
        let parsed = crate::CliCommand::parse(cli_arguments).expect("Host command parses");
        let crate::CliCommand::Host(command) = parsed else {
            panic!("expected parsed Host command, got {parsed:?}");
        };
        assert_eq!(
            resolve_host_ports(
                tempfile::tempdir().expect("root").path(),
                isolated_debug,
                command.port_overrides()
            )
            .expect("resolved launch input")
            .mcp_bind(),
            expected_bind,
            "{case}"
        );
    }
}

#[test]
fn explicit_host_port_overrides_keep_the_selected_port() {
    for port in [8787, 9876] {
        let command = HostCommand::parse(vec![
            OsString::from("--port"),
            OsString::from(port.to_string()),
        ])
        .expect("explicit Host port");

        assert_eq!(command.port, Some(port));
    }
}

#[test]
fn operator_deadlines_cover_their_owned_lifecycle_bounds() {
    assert!(
        operator_request_deadline(HostAction::Restart)
            > codex_router_host::APP_SERVER_SHUTDOWN_TIMEOUT,
        "whole-Host restart must outlive the app-server shutdown bound"
    );
    assert!(
        operator_request_deadline(HostAction::AppServer {
            action: AppServerAction::Restart
        }) > codex_router_host::APP_SERVER_SHUTDOWN_TIMEOUT,
        "app-server restart must outlive its complete shutdown bound"
    );
    assert!(
        operator_request_deadline(HostAction::AppServer {
            action: AppServerAction::Update
        }) > Duration::from_secs(15 * 60),
        "update transport must outlive the updater's own deadline"
    );
    assert_eq!(
        replacement_outcome::REPLACEMENT_CONVERGENCE_DEADLINE,
        Duration::from_secs(40),
        "replacement convergence starts only after old-host EOF"
    );
}

#[test]
fn provider_launch_arguments_require_and_retain_explicit_executables() {
    let command = HostCommand::parse(vec![
        OsString::from("--claude-acp-executable"),
        OsString::from("/tmp/claude-agent-acp"),
        OsString::from("--claude-acp-arguments"),
        OsString::from("--permission-mode"),
        OsString::from("--cursor-acp-executable"),
        OsString::from("/tmp/agent"),
        OsString::from("--cursor-acp-arguments"),
        OsString::from("acp"),
    ])
    .expect("provider launch arguments");

    let launches = external_provider_launches(&command).expect("validated launches");
    assert_eq!(launches.len(), 2);
    assert_eq!(launches[0].launch.arguments, vec!["--permission-mode"]);
    assert_eq!(launches[1].launch.arguments, vec!["acp"]);
}

#[test]
fn provider_operation_retention_is_positive_configurable_and_defaults_to_sixty_days() {
    let default = HostCommand::parse(Vec::new()).expect("default host command");
    assert_eq!(default.provider_operation_retention_days().get(), 60);
    let custom = HostCommand::parse(vec![
        OsString::from("--provider-operation-retention-days"),
        OsString::from("7"),
    ])
    .expect("custom retention");
    assert_eq!(custom.provider_operation_retention_days().get(), 7);
    assert!(
        HostCommand::parse(vec![
            OsString::from("--provider-operation-retention-days"),
            OsString::from("0"),
        ])
        .is_err()
    );
}

#[test]
fn owner_human_id_override_is_validated_at_cli_boundary() {
    let command = HostCommand::parse(vec![
        OsString::from("--owner-human-id"),
        OsString::from("chosen-owner"),
    ])
    .expect("valid owner id");
    assert_eq!(
        command
            .owner_human_id
            .as_ref()
            .map(message_board::HumanId::as_str),
        Some("chosen-owner")
    );
    assert!(
        HostCommand::parse(vec![OsString::from("--owner-human-id"), OsString::from("")]).is_err()
    );
}

#[test]
fn router_root_parent_segment_is_a_typed_error() {
    assert!(matches!(
        validate_router_root(Path::new("/tmp/private/../router")),
        Err(HostCommandError::RouterRootParentSegment)
    ));
}

#[tokio::test]
async fn claude_upstream_override_requires_debug_isolation_before_launch() {
    let context = CliContext::new(vec![(
        "CODEX_ROUTER_DEBUG_CLAUDE_UPSTREAM_BASE_URL".to_owned(),
        "http://127.0.0.1:18888".to_owned(),
    )]);
    let command = HostCommand::parse(Vec::new()).expect("default Host command");
    let error = run_host_command(&mut Vec::new(), command, &context, None)
        .await
        .expect_err("a Claude endpoint override requires isolated debug Host mode");

    assert!(matches!(
        error,
        HostCommandError::RouterRoot(message)
            if message == "debug Claude upstream override requires --require-debug-isolation in a debug build"
    ));
}
