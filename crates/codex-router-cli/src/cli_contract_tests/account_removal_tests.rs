use super::*;
use codex_router_secret_store::test_support::open_encrypted_credential_store;

struct RetainedRemovalCredentialResolver;
#[path = "account_removal_lifetime_tests.rs"]
mod lifetime_tests;

#[test]
fn account_remove_async_owner_refuses_a_mismatched_expected_record_without_deleting() {
    for mismatch in ["label", "provider", "generation"] {
        let fixture = RemovalFixture::new("remove-captured-mismatch", AccountStatus::Disabled);
        fixture.login_account("acct_secondary", "secondary");
        fixture.seed_dependents();
        let before = fixture.dependent_row_counts("acct_primary");
        let runtime = test_async_runtime();
        let result = runtime.block_on(async {
            let state =
                AsyncSqliteStateStore::open_read_only(&fixture.root.path().join("state.sqlite"))
                    .await
                    .unwrap();
            let actual = state
                .load_account(&account_id("acct_primary"))
                .await
                .unwrap()
                .unwrap();
            state.close().await.unwrap();
            let expected = AccountRecord::new(
                if mismatch == "provider" {
                    codex_router_core::provider::Provider::Claude
                } else {
                    actual.provider()
                },
                actual.account_id().clone(),
                if mismatch == "label" {
                    "different captured label"
                } else {
                    actual.label()
                },
                AccountStatus::Disabled,
            )
            .with_active_credential_generation(if mismatch == "generation" { 99 } else { 1 });
            crate::account::account_removal::remove_resolved_account(
                fixture.root.path().to_owned(),
                expected,
            )
            .await
        });
        assert!(
            matches!(
                result,
                Err(AccountCommandError::RemovalState(
                    codex_router_state::sqlite::AccountRemovalError::TargetChanged
                ))
            ),
            "{mismatch}: {result:?}"
        );
        assert!(fixture.account_exists());
        assert!(fixture.credential_exists(1));
        assert_eq!(fixture.dependent_row_counts("acct_primary"), before);
    }
}

impl AsyncProviderCredentialResolver for RetainedRemovalCredentialResolver {
    async fn resolve_provider_credentials_async(
        &self,
        account_id: &AccountId,
        _expected_provider: codex_router_core::provider::Provider,
    ) -> Result<ResolvedProviderCredential, CredentialResolverError> {
        Ok(ResolvedProviderCredential::new(
            account_id.clone(),
            SecretString::new("fixture-access"),
            1,
        ))
    }
}

struct HeldModelsQuotaProvider {
    entered: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
    response_succeeds: bool,
}

impl QuotaRefreshProvider for HeldModelsQuotaProvider {
    async fn fetch_quota(
        &self,
        request: QuotaRefreshProviderRequest,
    ) -> Result<
        QuotaRefreshProviderResponse,
        codex_router_quota_service::quota_service_error::QuotaServiceError,
    > {
        assert_eq!(request.account_id().as_str(), "acct_primary");
        if request.route_band() == "models" {
            self.entered.notify_one();
            self.release.notified().await;
            if !self.response_succeeds {
                return Err(codex_router_quota_service::quota_service_error::QuotaServiceError::ProviderStatus { status: 503 });
            }
        }
        Ok(QuotaRefreshProviderResponse {
            windows: verified_quota_windows(80),
            ..Default::default()
        })
    }
}

#[test]
fn account_remove_discards_in_flight_quota_writes() {
    let mut outcomes = Vec::new();
    for response_succeeds in [true, false] {
        let fixture = RemovalFixture::new("remove-in-flight-quota", AccountStatus::Enabled);
        fixture.login_account("acct_secondary", "secondary");
        // Keep the second account outside the refresh while preserving all of its rows.
        run_cli(
            [
                "account",
                "disable",
                "--account",
                "secondary",
                "--router-root",
                path_to_str(fixture.root.path()),
            ],
            CliContext::new(Vec::new()),
        );
        fixture.seed_dependents();
        let secondary_before = fixture.dependent_row_counts("acct_secondary");
        let state_path = fixture.root.path().join("state.sqlite");
        let secret_root = fixture.root.path().join("secrets");
        let runtime = test_async_runtime();
        let entered = Arc::new(tokio::sync::Notify::new());
        let release = Arc::new(tokio::sync::Notify::new());
        let provider = HeldModelsQuotaProvider {
            entered: Arc::clone(&entered),
            release: Arc::clone(&release),
            response_succeeds,
        };
        let mut refresh = runtime.spawn(async move {
            refresh_quota_store_paths_with_dependencies_async(
                &mut Vec::new(),
                &state_path,
                &secret_root,
                "unused-provider-endpoint".to_owned(),
                &RetainedRemovalCredentialResolver,
                &provider,
                1_100,
            )
            .await
        });
        runtime.block_on(async {
            tokio::select! {
                () = entered.notified() => {},
                result = &mut refresh => panic!("refresh ended before the held Models fetch: {result:?}"),
            }
        });
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
        assert_eq!(must_ok(fixture.remove()), "removed account: primary\n");
        release.notify_one();
        must_ok(must_ok(runtime.block_on(refresh)));
        assert!(!fixture.account_exists());
        assert!(!fixture.credential_exists(1));
        outcomes.push((
            response_succeeds,
            fixture.dependent_row_counts("acct_primary"),
            secondary_before,
            fixture.dependent_row_counts("acct_secondary"),
        ));
    }
    // Collect both late outcomes before asserting, so a red run exercises both paths.
    for (response_succeeds, removed_counts, secondary_before, secondary_after) in outcomes {
        assert_eq!(
            secondary_after, secondary_before,
            "second account must stay intact"
        );
        assert_eq!(
            removed_counts,
            vec![0; DEPENDENT_COUNTS.len()],
            "late response succeeds={response_succeeds}; tables={DEPENDENT_COUNTS:?}"
        );
    }
}

#[path = "account_removal_queued_tests.rs"]
mod queued_writes;

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
    for generation in [1, 2] {
        let fixture = RemovalFixture::new("remove-reenabled", AccountStatus::Disabled);
        let runtime = test_async_runtime();
        runtime.block_on(async {
            let state = must_ok(
                AsyncSqliteStateStore::open(&fixture.root.path().join("state.sqlite")).await,
            );
            let target =
                must_ok(state.load_account(&account_id("acct_primary")).await).expect("account");
            let reenabled = AccountRecord::new(
                target.provider(), target.account_id().clone(), target.label(), AccountStatus::Enabled,
            ).with_active_credential_generation(generation);
            must_ok(state.upsert_account(&reenabled).await);
            must_ok(state.close().await);
            let mutation = must_ok(
                AsyncWeeklyQuotaFloorMutationStore::open(&fixture.root.path().join("state.sqlite"))
                    .await,
            );
            let error = match mutation.begin_disabled_account_removal(&target).await {
                Ok(_) => panic!("an enabled target must be refused"),
                Err(error) => error,
            };
            assert!(matches!(
                &error,
                codex_router_state::sqlite::AccountRemovalError::RequiresDisabled { account_label }
                    if account_label == "primary"
            ), "generation {generation}: {error}");
            mutation.close().await;
        });
        assert!(fixture.account_exists());
        assert!(fixture.credential_exists(1));
    }
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

    fn dependent_row_counts(&self, account_name: &str) -> Vec<i64> {
        test_async_runtime().block_on(async {
            let pool = must_ok(
                sqlx::sqlite::SqlitePoolOptions::new()
                    .connect_with(
                        sqlx::sqlite::SqliteConnectOptions::new()
                            .filename(self.root.path().join("state.sqlite"))
                            .read_only(true),
                    )
                    .await,
            );
            let mut counts = Vec::new();
            for statement in DEPENDENT_COUNTS {
                counts.push(must_ok(
                    sqlx::query_scalar(*statement)
                        .bind(account_name)
                        .fetch_one(&pool)
                        .await,
                ));
            }
            pool.close().await;
            counts
        })
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
