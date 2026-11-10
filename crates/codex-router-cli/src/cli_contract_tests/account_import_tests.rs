use super::*;

#[test]
fn explicit_auth_file_account_entrypoints_are_unknown() {
    let login_error = must_err(CliCommand::parse([
        OsString::from("account"),
        OsString::from("login"),
        OsString::from("--label"),
        OsString::from("primary"),
        OsString::from("--auth-json"),
        OsString::from("/tmp/unused-auth.json"),
    ]));
    assert!(matches!(login_error, CliError::UnknownOption { option } if option == "--auth-json"));
    let import_error = must_err(CliCommand::parse([
        OsString::from("account"),
        OsString::from("import-codex-auth"),
    ]));
    assert!(
        matches!(import_error, CliError::UnknownCommand { command } if command == "account import-codex-auth")
    );
    let live_error = must_err(CliCommand::parse([
        OsString::from("live"),
        OsString::from("quota"),
        OsString::from("--auth-json"),
        OsString::from("/tmp/unused-auth.json"),
    ]));
    assert!(matches!(live_error, CliError::UnknownOption { option } if option == "--auth-json"));
}

#[tokio::test]
async fn openai_auth_import_refuses_an_existing_claude_account_before_writing() {
    let test_root = TestRoot::new("openai-import-provider-guard");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");

    let state = must_ok((AsyncSqliteStateStore::open(&state_path)).await);
    let account_id = account_id("claude-import-target");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Claude,
                    account_id.clone(),
                    "shared-label",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );

    let result = (import_codex_auth_from_request_async(
        &state,
        &secrets,
        AccountImportRequest::new(account_id.clone(), "shared-label", "fake-openai-access"),
    ))
    .await;

    assert!(
        result.is_err(),
        "OpenAI auth must not be written to a Claude account"
    );
    assert_eq!(
        must_ok(state.load_account(&account_id).await)
            .and_then(|account| account.active_credential_generation()),
        Some(1)
    );
    assert!(
        !secret_root
            .join("openai_credential_bundle.acct_claude-import-target.2.v2")
            .exists()
    );
}

#[derive(Clone)]
struct FailingWriteOnlySecretStore {
    write_attempts: Arc<AtomicUsize>,
}

impl SecretStore for FailingWriteOnlySecretStore {
    fn write_secret(
        &self,
        _key: &SecretKey,
        _secret: &SecretString,
    ) -> Result<(), SecretStoreError> {
        self.write_attempts.fetch_add(1, Ordering::SeqCst);
        Err(SecretStoreError::Filesystem {
            path: PathBuf::from("injected-secret-write"),
            source: std::io::Error::other("injected write failure"),
        })
    }

    fn read_secret(&self, _key: &SecretKey) -> Result<SecretString, SecretStoreError> {
        Err(SecretStoreError::Filesystem {
            path: PathBuf::from("unused-generation"),
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })
    }

    fn delete_staged(&self, _key: &SecretKey) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::KeyUnavailable)
    }
}

#[tokio::test]
async fn device_login_commit_preserves_old_secret_and_invalidates_quota() {
    let test_root = TestRoot::new("device-login-existing-credential");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");

    let state = must_ok((AsyncSqliteStateStore::open(&state_path)).await);
    let account_id = account_id("device-existing");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "device",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let old_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    let old_secret = must_ok(
        AccountCredentialBundle::imported_codex_auth(
            "old-access-canary",
            Some("old-refresh-canary".to_owned()),
        )
        .to_secret_string(),
    );
    must_ok(secrets.write_secret(&old_key, &old_secret));
    must_ok(
        state
            .upsert_quota_snapshot(
                &PersistedQuotaSnapshot::new(account_id.clone(), QuotaSnapshotSource::MockEndpoint)
                    .with_observed_unix_seconds(9_000)
                    .with_route_band("responses", 99),
            )
            .await,
    );
    must_ok(
        (import_codex_auth_from_request_async(
            &state,
            &secrets,
            AccountImportRequest::new(account_id.clone(), "device", "new-access-canary")
                .with_refresh_token("new-refresh-canary"),
        ))
        .await,
    );
    let account = must_ok(state.load_account(&account_id).await).expect("account should remain");
    assert_eq!(account.active_credential_generation(), Some(2));
    assert_eq!(
        must_ok(secrets.read_secret(&old_key)).expose_secret(),
        old_secret.expose_secret()
    );
    let new_key = must_ok(openai_account_credential_bundle_key(&account_id, 2));
    let new_bundle = must_ok(AccountCredentialBundle::from_secret_string(must_ok(
        secrets.read_secret(&new_key),
    )));
    assert_eq!(
        new_bundle.access_token().expose_secret(),
        "new-access-canary"
    );
    let snapshot = must_ok(
        state
            .load_quota_snapshot_for_route_band(&account_id, "responses")
            .await,
    )
    .expect("stale quota marker should remain");
    assert!(snapshot.stale_penalty());
    assert_eq!(snapshot.remaining_headroom(), 0);
}

#[derive(Clone)]
struct HeldLoginRaceRefreshClient {
    entered_sender: mpsc::Sender<()>,
    release_receiver: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl CredentialRefreshClient for HeldLoginRaceRefreshClient {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract asserts; setup may fail."
    )]
    fn refresh_credentials(
        &self,
        _account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, codex_router_auth::resolver::CredentialRefreshFailure>
    {
        assert_eq!(refresh_token.expose_secret(), "old-refresh-canary");
        self.entered_sender
            .send(())
            .unwrap_or_else(|_| panic!("provider should report entry"));
        self.release_receiver
            .lock()
            .unwrap_or_else(|_| panic!("release lock"))
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| panic!("provider should be released"));
        Ok(AccountCredentialBundle::imported_codex_auth(
            "rotated-access-canary",
            Some("rotated-refresh-canary".to_owned()),
        )
        .with_expires_unix_seconds(4_000_000_000))
    }
}

#[tokio::test]
async fn device_relogin_waits_for_claimed_refresh_then_wins_the_active_generation() {
    let test_root = TestRoot::new("device-relogin-refresh-race");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("device-refresh-race");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "race",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &active_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "expired-access-canary",
                    Some("old-refresh-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    state.close().await.expect("fixture state should close");
    let (entered_sender, entered_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let refresh_client = HeldLoginRaceRefreshClient {
        entered_sender,
        release_receiver: Arc::new(Mutex::new(release_receiver)),
    };
    let refresh_state_path = state_path.clone();
    let refresh_secret_root = secret_root.clone();
    let refresh_account = account_id.clone();
    let refresh_thread = thread::spawn(move || {
        let resolver = must_ok(CliCredentialResolver::open_with_refresh_client(
            &refresh_state_path,
            &refresh_secret_root,
            refresh_client,
        ));
        must_ok(resolver.resolve_provider_credentials(
            &refresh_account,
            codex_router_core::provider::Provider::Openai,
        ))
        .credential_generation()
    });
    must_ok(entered_receiver.recv_timeout(Duration::from_secs(2)));
    let login_state_path = state_path.clone();
    let login_secret_root = secret_root;
    let login_account = account_id.clone();
    let login = async move {
        let state = must_ok((AsyncSqliteStateStore::open(&login_state_path)).await);
        let secrets = must_ok(
            codex_router_secret_store::test_support::open_encrypted_credential_store(
                &login_secret_root,
            ),
        );
        must_ok(
            (import_codex_auth_from_request_async(
                &state,
                &secrets,
                AccountImportRequest::new(login_account, "race", "login-access-canary")
                    .with_refresh_token("login-refresh-canary"),
            ))
            .await,
        );
        must_ok(state.close().await);
    };
    tokio::pin!(login);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut login)
            .await
            .is_err()
    );
    must_ok(release_sender.send(()));
    assert_eq!(
        must_ok(refresh_thread.join().map_err(|_| "refresh thread failed")),
        2
    );
    must_ok(tokio::time::timeout(Duration::from_secs(2), login).await);
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let active = must_ok(state.load_account(&account_id).await).expect("account should remain");
    assert_eq!(active.active_credential_generation(), Some(3));
    let login_key = must_ok(openai_account_credential_bundle_key(&account_id, 3));
    let login_bundle = must_ok(AccountCredentialBundle::from_secret_string(must_ok(
        secrets.read_secret(&login_key),
    )));
    assert_eq!(
        login_bundle.access_token().expose_secret(),
        "login-access-canary"
    );
}

#[tokio::test]
async fn refresh_claim_cannot_reenable_an_account_disabled_during_provider_refresh() {
    let test_root = TestRoot::new("refresh-disabled-account-race");
    must_ok(fs::create_dir(test_root.path()));
    let state_path = codex_router_state::provider_proxy_database_path(test_root.path());
    let secret_root = test_root.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&state_path).await);
    let account_id = account_id("refresh-disable-race");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "refresh disable race",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_root),
    );
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &active_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "disable-race-expired-access-canary",
                    Some("old-refresh-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    state.close().await.expect("fixture state should close");

    let (entered_sender, entered_receiver) = mpsc::channel();
    let (release_sender, release_receiver) = mpsc::channel();
    let refresh_client = HeldLoginRaceRefreshClient {
        entered_sender,
        release_receiver: Arc::new(Mutex::new(release_receiver)),
    };
    let refresh_state_path = state_path.clone();
    let refresh_secret_root = secret_root;
    let refresh_account = account_id.clone();
    let refresh_thread = thread::spawn(move || {
        let resolver = must_ok(CliCredentialResolver::open_with_refresh_client(
            &refresh_state_path,
            &refresh_secret_root,
            refresh_client,
        ));
        resolver
            .resolve_provider_credentials(
                &refresh_account,
                codex_router_core::provider::Provider::Openai,
            )
            .is_err()
    });
    must_ok(entered_receiver.recv_timeout(Duration::from_secs(2)));

    let disable_state = must_ok((AsyncSqliteStateStore::open(&state_path)).await);
    must_ok(
        disable_state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "refresh disable race",
                    AccountStatus::Disabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    must_ok(release_sender.send(()));

    assert!(must_ok(
        refresh_thread.join().map_err(|_| "refresh thread failed")
    ));
    let disabled_account = must_ok(disable_state.load_account(&account_id).await)
        .expect("disabled account should remain registered");
    assert_eq!(disabled_account.status(), AccountStatus::Disabled);
    assert_eq!(disabled_account.active_credential_generation(), Some(1));
}

#[tokio::test]
async fn failed_device_login_secret_commit_leaves_new_account_disabled() {
    let test_root = TestRoot::new("device-login-write-failure");
    must_ok(fs::create_dir(test_root.path()));

    let state = must_ok(
        (AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            test_root.path(),
        )))
        .await,
    );
    let account_id = account_id("device-write-failure");
    let secrets = FailingWriteOnlySecretStore {
        write_attempts: Arc::new(AtomicUsize::new(0)),
    };
    let error = must_err(
        (import_codex_auth_from_request_async(
            &state,
            &secrets,
            AccountImportRequest::new(account_id.clone(), "device", "new-access-canary")
                .with_refresh_token("new-refresh-canary"),
        ))
        .await,
    );
    assert!(error.to_string().contains("secret store"));
    let account =
        must_ok(state.load_account(&account_id).await).expect("new account metadata should remain");
    assert_eq!(account.status(), AccountStatus::Disabled);
    assert_eq!(account.active_credential_generation(), None);
    assert_eq!(secrets.write_attempts.load(Ordering::SeqCst), 1);
}
