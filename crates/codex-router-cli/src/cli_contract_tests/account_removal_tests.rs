use super::*;
use codex_router_secret_store::test_support::open_encrypted_credential_store;

#[test]
fn account_remove_help_and_target_options_follow_sibling_commands() {
    let help = must_ok(CliCommand::parse(
        ["account", "remove", "--help"].map(OsString::from),
    ));
    assert!(
        matches!(help, CliCommand::Account(AccountCommand::Help(text)) if text.contains("Disable it first") && text.contains("account login"))
    );
    for arguments in [
        vec!["account", "remove"],
        vec![
            "account",
            "remove",
            "--account",
            "primary",
            "--account",
            "secondary",
        ],
        vec!["account", "remove", "--account", "primary", "--json"],
    ] {
        assert!(CliCommand::parse(arguments.into_iter().map(OsString::from)).is_err());
    }
}

#[test]
fn account_remove_refuses_enabled_account_without_changing_credentials_or_rows() {
    let fixture = RemovalFixture::new("remove-enabled", AccountStatus::Enabled);

    let error = fixture
        .remove()
        .expect_err("enabled accounts require disable first");

    assert!(error.to_string().contains("primary"));
    assert!(error.to_string().contains("disable"));
    assert!(fixture.account_exists());
    assert!(fixture.credential_exists(1));
}

#[test]
fn account_remove_disabled_account_deletes_all_generations_and_dependent_rows() {
    let fixture = RemovalFixture::new("remove-disabled", AccountStatus::Disabled);
    fixture.login_account("acct_secondary", "secondary");
    let secrets = must_ok(open_encrypted_credential_store(
        fixture.root.path().join("secrets"),
    ));
    for generation in [2, 9] {
        let key = must_ok(openai_account_credential_bundle_key(
            &account_id("acct_primary"),
            generation,
        ));
        must_ok(secrets.write_staged(&key, &SecretString::new("fixture-staged-credential")));
    }
    fixture.seed_dependents();

    assert_eq!(must_ok(fixture.remove()), "removed account: primary\n");

    assert!(!fixture.account_exists());
    for generation in [1, 2, 9] {
        assert!(!fixture.credential_exists(generation));
    }
    fixture.assert_no_dependent_rows();
    let secondary_key = must_ok(openai_account_credential_bundle_key(
        &account_id("acct_secondary"),
        1,
    ));
    assert!(secrets.read_secret(&secondary_key).is_ok());
    let listed = run_cli(
        [
            "account",
            "list",
            "--router-root",
            path_to_str(fixture.root.path()),
        ],
        CliContext::new(Vec::new()),
    );
    assert!(!listed.stdout.contains("primary"));
}

#[test]
fn account_remove_second_attempt_reports_not_found() {
    let fixture = RemovalFixture::new("remove-twice", AccountStatus::Disabled);
    must_ok(fixture.remove());

    let error = fixture
        .remove()
        .expect_err("a second removal must report not found");

    assert!(error.to_string().contains("not found"));
}

#[test]
fn account_remove_allows_login_with_the_same_label_again() {
    let fixture = RemovalFixture::new("remove-login", AccountStatus::Disabled);
    must_ok(fixture.remove());

    let issuer = super::account_login_tests::FakeDeviceIssuer::spawn("new-access", "new-refresh");
    let client = codex_router_auth::openai_oauth::OpenAiOAuthDeviceLoginClient::with_test_issuer(
        issuer.base_url.clone(),
        Duration::from_secs(60),
    );
    let command = must_ok(CliCommand::parse(
        [
            "account",
            "login",
            "--label",
            "primary",
            "--router-root",
            path_to_str(fixture.root.path()),
        ]
        .map(OsString::from),
    ));
    let CliCommand::Account(command) = command else {
        panic!("expected account login");
    };
    let mut stdout = Vec::new();
    must_ok(
        crate::account::run_account_command_with_input_and_openai_client(
            &mut stdout,
            &mut std::io::Cursor::new(Vec::<u8>::new()),
            command,
            &client,
        ),
    );
    assert_eq!(issuer.finish().len(), 3);
    assert!(must_ok(String::from_utf8(stdout)).contains("logged in account: primary"));

    assert!(fixture.account_exists());
    assert!(fixture.credential_exists(1));
}

#[test]
fn account_remove_preserves_disabled_metadata_when_secret_cleanup_fails() {
    let fixture = RemovalFixture::new("remove-cleanup-failure", AccountStatus::Disabled);
    let credential_path = fixture
        .root
        .path()
        .join("secrets/openai_credential_bundle.acct_primary.1.v2");
    must_ok(fs::rename(
        &credential_path,
        credential_path.with_extension("retained"),
    ));
    must_ok(fs::create_dir(&credential_path));

    let error = fixture
        .remove()
        .expect_err("an invalid secret entry must stop removal");

    assert!(error.to_string().contains("remains disabled"));
    assert!(fixture.account_exists());
}

#[test]
fn account_remove_rechecks_disabled_state_after_target_resolution() {
    let fixture = RemovalFixture::new("remove-reenabled", AccountStatus::Disabled);
    let runtime = test_async_runtime();
    runtime.block_on(async {
        let state = must_ok(
            AsyncSqliteStateStore::open_read_only(&fixture.root.path().join("state.sqlite")).await,
        );
        let target =
            must_ok(state.load_account(&account_id("acct_primary")).await).expect("account");
        must_ok(state.close().await);
        let mutation = must_ok(
            AsyncWeeklyQuotaFloorMutationStore::open(&fixture.root.path().join("state.sqlite"))
                .await,
        );
        must_ok(
            mutation
                .set_account_status_by_label("primary", AccountStatus::Enabled)
                .await,
        );
        let result = mutation.begin_disabled_account_removal(&target).await;
        assert!(matches!(
            result,
            Err(codex_router_state::sqlite::AccountRemovalError::RequiresDisabled { .. })
        ));
        mutation.close().await;
    });
    assert!(fixture.account_exists());
    assert!(fixture.credential_exists(1));
}

struct RemovalFixture {
    root: TestRoot,
}

impl RemovalFixture {
    fn new(name: &str, status: AccountStatus) -> Self {
        let fixture = Self {
            root: TestRoot::new(name),
        };
        must_ok(fs::create_dir_all(fixture.root.path()));
        fixture.login();
        if status == AccountStatus::Disabled {
            run_cli(
                [
                    "account",
                    "disable",
                    "--account",
                    "primary",
                    "--router-root",
                    path_to_str(fixture.root.path()),
                ],
                CliContext::new(Vec::new()),
            );
        }
        fixture
    }

    fn login(&self) {
        self.login_account("acct_primary", "primary");
    }

    fn login_account(&self, account_name: &str, label: &str) {
        let runtime = test_async_runtime();
        let state = must_ok(runtime.block_on(AsyncSqliteStateStore::open(
            &self.root.path().join("state.sqlite"),
        )));
        let secrets = must_ok(open_encrypted_credential_store(
            self.root.path().join("secrets"),
        ));
        must_ok(
            runtime.block_on(import_codex_auth_from_request_async(
                &state,
                &secrets,
                AccountImportRequest::new(account_id(account_name), label, "fixture-access")
                    .with_refresh_token("fixture-refresh"),
            )),
        );
        must_ok(runtime.block_on(state.close()));
    }

    #[expect(
        clippy::panic_in_result_fn,
        reason = "CLI errors are returned for rejection assertions; empty stderr is a separate test contract."
    )]
    fn remove(&self) -> Result<String, CliError> {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        run_with_io(
            [
                "account",
                "remove",
                "--account",
                "primary",
                "--router-root",
                path_to_str(self.root.path()),
            ]
            .map(OsString::from)
            .to_vec(),
            &CliContext::new(Vec::new()),
            &mut stdout,
            &mut stderr,
        )?;
        assert!(stderr.is_empty());
        Ok(must_ok(String::from_utf8(stdout)))
    }

    fn account_exists(&self) -> bool {
        let runtime = test_async_runtime();
        let state = must_ok(runtime.block_on(AsyncSqliteStateStore::open_read_only(
            &self.root.path().join("state.sqlite"),
        )));
        let exists =
            must_ok(runtime.block_on(state.load_account(&account_id("acct_primary")))).is_some();
        must_ok(runtime.block_on(state.close()));
        exists
    }

    fn credential_exists(&self, generation: u64) -> bool {
        let secrets = must_ok(open_encrypted_credential_store(
            self.root.path().join("secrets"),
        ));
        let key = must_ok(openai_account_credential_bundle_key(
            &account_id("acct_primary"),
            generation,
        ));
        secrets.read_secret(&key).is_ok()
    }

    fn seed_dependents(&self) {
        let runtime = test_async_runtime();
        runtime.block_on(async {
            let state =
                must_ok(AsyncSqliteStateStore::open(&self.root.path().join("state.sqlite")).await);
            let pool = must_ok(
                sqlx::SqlitePool::connect(&format!(
                    "sqlite://{}",
                    self.root.path().join("state.sqlite").display()
                ))
                .await,
            );
            for account_name in ["acct_primary", "acct_secondary"] {
                for statement in DEPENDENT_INSERTS {
                    must_ok(
                        sqlx::query(*statement)
                            .bind(account_name)
                            .execute(&pool)
                            .await,
                    );
                }
                // Allocate through the owning state API so every required observation field
                // comes from the real storage contract rather than a partial fixture row.
                must_ok(
                    state
                        .begin_credit_refresh_attempt(&account_id(account_name), 1)
                        .await,
                );
            }
            pool.close().await;
            must_ok(state.close().await);
        });
    }

    fn assert_no_dependent_rows(&self) {
        let runtime = test_async_runtime();
        runtime.block_on(async {
            let pool = must_ok(
                sqlx::SqlitePool::connect(&format!(
                    "sqlite://{}",
                    self.root.path().join("state.sqlite").display()
                ))
                .await,
            );
            for statement in DEPENDENT_COUNTS {
                let count: i64 = must_ok(
                    sqlx::query_scalar(*statement)
                        .bind("acct_primary")
                        .fetch_one(&pool)
                        .await,
                );
                assert_eq!(count, 0, "removed account: {statement}");
                let count: i64 = must_ok(
                    sqlx::query_scalar(*statement)
                        .bind("acct_secondary")
                        .fetch_one(&pool)
                        .await,
                );
                assert!(count > 0, "unrelated account must survive: {statement}");
            }
            pool.close().await;
        });
    }
}

const DEPENDENT_INSERTS: &[&str] = &[
    "INSERT OR REPLACE INTO quota_snapshots (account_id, source, observed_unix_seconds, route_band, remaining_headroom, stale_penalty) VALUES (?1, 'mock_endpoint', 1, 'responses', 90, 0)",
    "INSERT INTO affinity_pins VALUES (?1, ?1)",
    "INSERT INTO selector_quota_windows VALUES (?1, 'responses', 300, 'eligible', 90, NULL, 1, 1)",
    "INSERT INTO quota_refresh_status (account_id, route_band) VALUES (?1, 'responses')",
    "INSERT INTO previous_response_affinity_owners VALUES (?1, 'responses', ?1, 1, 'http', 1)",
    "INSERT INTO quota_history_observations (account_id, account_label, route_band, limit_window_seconds, observed_unix_seconds, remaining_headroom, window_status, effective, refresh_source, refresh_success) VALUES (?1, ?1, 'responses', 300, 1, 90, 'eligible', 1, 'mock_endpoint', 1)",
    "INSERT INTO active_client_leases VALUES ('responses', 'fixture-run', ?1, ?1, 1, 1)",
    "INSERT INTO route_band_account_states VALUES (?1, 'responses', 'eligible', 'none', 1, NULL)",
    "INSERT INTO active_session_events (account_id, route_band, process_run_id, logical_session_id, reservation_id, event_kind, event_unix_seconds, session_started_unix_seconds, transport_kind) VALUES (?1, 'responses', 'fixture-run', ?1, ?1, 'started', 1, 1, 'http')",
    "INSERT INTO active_session_rollups VALUES (?1, 'responses', 1, 2, 1, 1, 0, 0)",
    "INSERT INTO account_routing_policies VALUES (?1, 500)",
    "INSERT INTO session_account_affinities VALUES ('openai', ?1, ?1, 1, 0)",
    "INSERT INTO credential_maintenance (account_id, credential_generation, state) VALUES (?1, 1, 'healthy')",
    "INSERT INTO account_window_observations (account_id, window_kind, remaining_basis_points, observation_started_at) VALUES (?1, 'five_hour', 9000, 1)",
    "INSERT INTO account_window_rejections (account_id, window_kind, rejected_at) VALUES (?1, 'five_hour', 1)",
    "INSERT INTO account_credit_policies VALUES (?1, 0)",
];

const DEPENDENT_COUNTS: &[&str] = &[
    "SELECT COUNT(*) FROM quota_snapshots WHERE account_id = ?1",
    "SELECT COUNT(*) FROM affinity_pins WHERE account_id = ?1",
    "SELECT COUNT(*) FROM selector_quota_windows WHERE account_id = ?1",
    "SELECT COUNT(*) FROM quota_refresh_status WHERE account_id = ?1",
    "SELECT COUNT(*) FROM previous_response_affinity_owners WHERE account_id = ?1",
    "SELECT COUNT(*) FROM quota_history_observations WHERE account_id = ?1",
    "SELECT COUNT(*) FROM active_client_leases WHERE account_id = ?1",
    "SELECT COUNT(*) FROM route_band_account_states WHERE account_id = ?1",
    "SELECT COUNT(*) FROM active_session_events WHERE account_id = ?1",
    "SELECT COUNT(*) FROM active_session_rollups WHERE account_id = ?1",
    "SELECT COUNT(*) FROM account_routing_policies WHERE account_id = ?1",
    "SELECT COUNT(*) FROM session_account_affinities WHERE account_id = ?1",
    "SELECT COUNT(*) FROM credential_maintenance WHERE account_id = ?1",
    "SELECT COUNT(*) FROM account_window_observations WHERE account_id = ?1",
    "SELECT COUNT(*) FROM account_window_rejections WHERE account_id = ?1",
    "SELECT COUNT(*) FROM account_credit_policies WHERE account_id = ?1",
    "SELECT COUNT(*) FROM account_credit_observations WHERE account_id = ?1",
];
