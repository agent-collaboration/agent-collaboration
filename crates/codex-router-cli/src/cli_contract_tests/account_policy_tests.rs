use super::*;

#[test]
fn account_list_command_defaults_to_home_router_root() {
    let command = match CliCommand::parse([OsString::from("account"), OsString::from("list")]) {
        Ok(CliCommand::Account(command)) => command,
        Ok(other) => panic!("account command should parse, got {other:?}"),
        Err(error) => panic!("account command should parse: {error}"),
    };

    let AccountCommand::List { router_root } = command else {
        panic!("account list command should parse");
    };
    assert_eq!(router_root, default_router_root_for_test());
}

#[test]
fn account_set_weekly_floor_parses_exact_contract() {
    let router_root = PathBuf::from("/tmp/codex-router-weekly-floor");
    let command = match CliCommand::parse([
        OsString::from("account"),
        OsString::from("set-weekly-floor"),
        OsString::from("--account"),
        OsString::from("primary"),
        OsString::from("--percent"),
        OsString::from("15"),
        OsString::from("--router-root"),
        router_root.clone().into_os_string(),
    ]) {
        Ok(CliCommand::Account(command)) => command,
        Ok(other) => panic!("account command should parse, got {other:?}"),
        Err(error) => panic!("weekly-floor command should parse: {error}"),
    };

    let AccountCommand::SetWeeklyFloor {
        router_root: parsed_root,
        account_label,
        percent,
    } = command
    else {
        panic!("weekly-floor command should parse");
    };
    assert_eq!(parsed_root, router_root);
    assert_eq!(account_label, "primary");
    assert_eq!(percent, 15);

    let help = match CliCommand::parse([
        OsString::from("account"),
        OsString::from("set-weekly-floor"),
        OsString::from("--help"),
    ]) {
        Ok(CliCommand::Account(AccountCommand::Help(help))) => help,
        Ok(other) => panic!("weekly-floor help should parse, got {other:?}"),
        Err(error) => panic!("weekly-floor help should parse: {error}"),
    };
    assert!(help.contains("--percent <0-15>"));
}

#[tokio::test]
async fn account_status_commands_parse_and_preserve_credential_generation() {
    let test_root = TestRoot::new("account-status");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id("acct_primary"),
                    "primary".to_owned(),
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(7),
            )
            .await,
    );
    state.close().await.expect("fixture state should close");
    migrate_test_state_database(&codex_router_state::provider_proxy_database_path(
        &router_root,
    ))
    .await;

    for (command, expected_status) in [
        ("disable", AccountStatus::Disabled),
        ("enable", AccountStatus::Enabled),
    ] {
        let output = run_cli(
            [
                "account",
                command,
                "--account",
                "primary",
                "--router-root",
                path_to_str(&router_root),
            ],
            CliContext::new(Vec::new()),
        )
        .await;
        assert_eq!(
            output.stdout,
            format!(
                "updated account status: primary = {}\n",
                expected_status.as_str()
            )
        );
        assert!(output.stderr.is_empty());

        let state = must_ok(
            (AsyncSqliteStateStore::open_read_only(
                &codex_router_state::provider_proxy_database_path(&router_root),
            ))
            .await,
        );
        let account = must_ok(state.load_account(&account_id("acct_primary")).await)
            .unwrap_or_else(|| panic!("status command must retain account metadata"));
        assert_eq!(account.status(), expected_status);
        assert_eq!(account.active_credential_generation(), Some(7));
    }
}

#[test]
fn account_status_commands_require_one_account_option() {
    for arguments in [
        vec!["account", "disable"],
        vec![
            "account",
            "enable",
            "--account",
            "primary",
            "--account",
            "secondary",
        ],
    ] {
        let result = CliCommand::parse(arguments.into_iter().map(OsString::from));
        assert!(
            result.is_err(),
            "status commands require one account option"
        );
    }
}

#[test]
fn account_status_command_help_is_specific() {
    for (command, expected_line) in [
        ("disable", "codex-router account disable --account <label>"),
        ("enable", "codex-router account enable --account <label>"),
    ] {
        let parsed = CliCommand::parse([
            OsString::from("account"),
            OsString::from(command),
            OsString::from("--help"),
        ]);
        let Ok(CliCommand::Account(AccountCommand::Help(help))) = parsed else {
            panic!("{command} help should parse");
        };
        assert!(help.contains(expected_line));
    }
}

#[test]
fn account_set_weekly_floor_rejects_invalid_and_duplicate_options() {
    for invalid_percent in ["-1", "2.5", "16", "65536"] {
        let result = CliCommand::parse([
            OsString::from("account"),
            OsString::from("set-weekly-floor"),
            OsString::from("--account"),
            OsString::from("primary"),
            OsString::from("--percent"),
            OsString::from(invalid_percent),
        ]);
        assert!(result.is_err(), "{invalid_percent} must be rejected");
    }

    for arguments in [
        vec!["account", "set-weekly-floor", "--percent", "5"],
        vec!["account", "set-weekly-floor", "--account", "primary"],
        vec![
            "account",
            "set-weekly-floor",
            "--account",
            "primary",
            "--account",
            "secondary",
            "--percent",
            "5",
        ],
        vec![
            "account",
            "set-weekly-floor",
            "--account",
            "primary",
            "--percent",
            "5",
            "--percent",
            "6",
        ],
    ] {
        let result = CliCommand::parse(arguments.into_iter().map(OsString::from));
        assert!(result.is_err(), "missing or duplicate options must fail");
    }
}

#[test]
fn account_set_weekly_floor_busy_error_is_exact_and_redacted() {
    let rendered = AccountCommandError::WeeklyFloorDatabaseBusy.to_string();
    assert_eq!(
        rendered,
        "failed to update weekly floor: database is busy; retry the command"
    );
    for canary in ["sensitive-label", "acct_internal", "/private/state.sqlite"] {
        assert!(!rendered.contains(canary));
    }
}

#[tokio::test]
async fn account_set_weekly_floor_updates_disables_and_lists_policy() {
    let test_root = TestRoot::new("account-weekly-floor");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    must_ok(
        state
            .upsert_account(&AccountRecord::new(
                codex_router_core::provider::Provider::Openai,
                account_id("acct_primary"),
                "primary".to_owned(),
                AccountStatus::Enabled,
            ))
            .await,
    );
    state.close().await.expect("fixture state should close");
    migrate_test_state_database(&codex_router_state::provider_proxy_database_path(
        &router_root,
    ))
    .await;

    let enabled = run_cli(
        [
            "account",
            "set-weekly-floor",
            "--account",
            "primary",
            "--percent",
            "5",
            "--router-root",
            path_to_str(&router_root),
        ],
        CliContext::new(Vec::new()),
    )
    .await;
    assert_eq!(enabled.stdout, "updated weekly floor: primary = 5%\n");
    assert!(enabled.stderr.is_empty());

    let listed = run_cli(
        [
            "account",
            "list",
            "--router-root",
            path_to_str(&router_root),
        ],
        CliContext::new(Vec::new()),
    )
    .await;
    assert!(listed.stdout.contains("weekly floor"));
    assert!(listed.stdout.contains("5%"));
    assert!(!listed.stdout.contains("acct_primary"));

    let disabled = run_cli(
        [
            "account",
            "set-weekly-floor",
            "--account",
            "primary",
            "--percent",
            "0",
            "--router-root",
            path_to_str(&router_root),
        ],
        CliContext::new(Vec::new()),
    )
    .await;
    assert_eq!(
        disabled.stdout,
        "updated weekly floor: primary = disabled (0%)\n"
    );
    assert!(disabled.stderr.is_empty());
}

#[tokio::test]
async fn account_set_weekly_floor_label_failures_are_redacted_and_do_not_mutate() {
    let test_root = TestRoot::new("account-weekly-floor-labels");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    for (account_id_value, label) in [
        ("acct_duplicate_a", "duplicate"),
        ("acct_duplicate_b", "duplicate"),
    ] {
        must_ok(
            state
                .upsert_account(&AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id(account_id_value),
                    label.to_owned(),
                    AccountStatus::Enabled,
                ))
                .await,
        );
    }
    state.close().await.expect("fixture state should close");
    migrate_test_state_database(&codex_router_state::provider_proxy_database_path(
        &router_root,
    ))
    .await;

    for supplied_label in ["missing-sensitive-label", "duplicate"] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let result = {
            let command_arguments = ([
                "account",
                "set-weekly-floor",
                "--account",
                supplied_label,
                "--percent",
                "5",
                "--router-root",
                path_to_str(&router_root),
            ]
            .into_iter()
            .map(OsString::from))
            .collect::<Vec<OsString>>();
            let command_argument_1 = (CliContext::new(Vec::new())).clone();
            let command_dispatcher = tracing::dispatcher::get_default(Clone::clone);
            let (command_result, command_stdout, command_stderr) = must_ok(
                tokio::task::spawn_blocking(move || {
                    tracing::dispatcher::with_default(&command_dispatcher, || {
                        let mut stdout = stdout;
                        let mut stderr = stderr;
                        let command_result = run_with_io(
                            command_arguments,
                            &command_argument_1,
                            &mut stdout,
                            &mut stderr,
                        );
                        (command_result, stdout, stderr)
                    })
                })
                .await,
            );
            stdout = command_stdout;
            stderr = command_stderr;
            command_result
        };
        let error = result.expect_err("missing or ambiguous label must fail");
        let rendered = error.to_string();
        assert!(stdout.is_empty());
        assert!(stderr.is_empty());
        assert!(!rendered.contains(supplied_label));
        assert!(!rendered.contains("acct_duplicate"));
    }

    let state = must_ok(
        (AsyncSqliteStateStore::open_read_only(&codex_router_state::provider_proxy_database_path(
            &router_root,
        )))
        .await,
    );
    let policies = must_ok((state.list_account_routing_policies()).await);
    assert!(policies.is_empty(), "failed label lookup must not mutate");
}

#[tokio::test]
async fn account_list_renders_friendly_table_without_account_ids() {
    let test_root = TestRoot::new("account-list-table");
    must_ok(fs::create_dir(test_root.path()));
    let router_root = test_root.path().join("router");
    must_ok(fs::create_dir(&router_root));
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            &router_root,
        ))
        .await,
    );
    must_ok(
        state
            .upsert_account(&AccountRecord::new(
                codex_router_core::provider::Provider::Openai,
                account_id("acct_primary"),
                "primary".to_owned(),
                AccountStatus::Enabled,
            ))
            .await,
    );
    state.close().await.expect("fixture state should close");
    migrate_test_state_database(&codex_router_state::provider_proxy_database_path(
        &router_root,
    ))
    .await;

    let output = run_cli(
        [
            "account",
            "list",
            "--router-root",
            path_to_str(&router_root),
        ],
        CliContext::new(Vec::new()),
    )
    .await;

    assert!(output.stdout.contains("account"));
    assert!(output.stdout.contains("provider"));
    assert!(output.stdout.contains("status"));
    assert!(output.stdout.contains("primary"));
    assert!(output.stdout.contains("enabled"));
    assert!(!output.stdout.contains("acct_primary"));
    assert!(output.stderr.is_empty());
}
