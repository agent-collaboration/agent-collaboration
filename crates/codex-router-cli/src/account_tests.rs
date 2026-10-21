use super::*;

#[cfg(test)]
mod account_status_error_tests {
    use super::*;

    #[test]
    fn schema_upgrade_error_is_actionable_and_redacted() {
        let rendered = redacted_account_status_state_error(
            StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired,
        )
        .to_string();
        assert_eq!(
            rendered,
            "account status requires a compatible upgraded router database"
        );
        for canary in ["sensitive-label", "acct_internal", "/private/state.sqlite"] {
            assert!(!rendered.contains(canary));
        }
    }
}

#[cfg(test)]
mod account_provider_cli_tests {
    use super::*;
    use codex_router_core::provider::Provider;

    #[test]
    fn login_rejects_mixed_provider_duplicate_labels() {
        assert_duplicate_login_labels_rejected(Provider::Openai, Provider::Claude);
    }

    #[test]
    fn login_rejects_same_provider_duplicate_labels() {
        for provider in [Provider::Openai, Provider::Claude] {
            assert_duplicate_login_labels_rejected(provider, provider);
        }
    }

    fn assert_duplicate_login_labels_rejected(first: Provider, second: Provider) {
        // Arrange: the store admits two IDs sharing an exact display label.
        let root = tempfile::tempdir().unwrap();
        let runtime = account_command_runtime().unwrap();
        let state = runtime
            .block_on(AsyncSqliteStateStore::open(
                &root.path().join("state.sqlite"),
            ))
            .unwrap();
        for (id, provider) in [("acct_a_existing", first), ("acct_b_existing", second)] {
            runtime
                .block_on(state.upsert_account(&AccountRecord::new(
                    provider,
                    AccountId::new(id).unwrap(),
                    "ambiguous label",
                    AccountStatus::Enabled,
                )))
                .unwrap();
        }
        assert_eq!(runtime.block_on(state.list_accounts()).unwrap().len(), 2);
        runtime.block_on(state.close()).unwrap();

        // Act/assert: login preflight refuses ambiguity for either provider.
        for provider in [Provider::Openai, Provider::Claude] {
            assert!(
                matches!(
                    resolve_login_account_id_at_router_root(
                        root.path(),
                        "ambiguous label",
                        provider
                    ),
                    Err(AccountCommandError::DuplicateAccountLabel { .. })
                ),
                "duplicate labels must never select the first matching account"
            );
        }
    }

    #[test]
    fn new_login_label_derives_identity_and_rejects_another_accounts_id() {
        // Arrange: a different label already owns the normalized ID.
        let root = tempfile::tempdir().unwrap();
        let runtime = account_command_runtime().unwrap();
        let state = runtime
            .block_on(AsyncSqliteStateStore::open(
                &root.path().join("state.sqlite"),
            ))
            .unwrap();
        for provider in [Provider::Openai, Provider::Claude] {
            assert_eq!(
                resolve_login_account_id(&state, "brand new", provider, &runtime).unwrap(),
                AccountId::new("acct_brand_new").unwrap()
            );
        }
        runtime
            .block_on(state.upsert_account(&AccountRecord::new(
                Provider::Openai,
                AccountId::new("acct_brand_new").unwrap(),
                "unrelated label",
                AccountStatus::Enabled,
            )))
            .unwrap();

        // Act/assert: neither provider may activate into another label's ID.
        for provider in [Provider::Openai, Provider::Claude] {
            assert!(matches!(
                resolve_login_account_id(&state, "brand new", provider, &runtime),
                Err(AccountCommandError::DuplicateAccountLabel { .. })
            ));
        }
        runtime.block_on(state.close()).unwrap();
    }

    #[test]
    fn login_refuses_a_label_owned_by_another_provider_before_device_request() {
        let temporary_root = tempfile::tempdir().expect("temporary router root should exist");
        let runtime = account_command_runtime().expect("test runtime should initialize");
        let state = runtime
            .block_on(AsyncSqliteStateStore::open(
                &temporary_root.path().join("state.sqlite"),
            ))
            .expect("test state should open");
        let existing_account = AccountRecord::new(
            Provider::Claude,
            account_id_from_label("shared-label").expect("test account id should parse"),
            "shared-label",
            AccountStatus::Enabled,
        );
        runtime
            .block_on(state.upsert_account(&existing_account))
            .expect("Claude account should persist");
        runtime
            .block_on(state.close())
            .expect("test state should close");

        let client = OpenAiOAuthDeviceLoginClient::with_test_issuer(
            "http://127.0.0.1:1",
            std::time::Duration::from_secs(1),
        );
        let error = login_with_openai_device_auth(
            &mut Vec::new(),
            temporary_root.path().to_path_buf(),
            " shared-label ".to_owned(),
            &client,
            None,
        )
        .expect_err("duplicate labels must be refused before device auth");
        assert!(matches!(
            error,
            AccountCommandError::DuplicateAccountLabel { label } if label == "shared-label"
        ));
    }

    #[test]
    fn account_list_displays_each_provider() {
        let temporary_root = tempfile::tempdir().expect("temporary router root should exist");
        let runtime = account_command_runtime().expect("test runtime should initialize");
        let state = runtime
            .block_on(AsyncSqliteStateStore::open(
                &temporary_root.path().join("state.sqlite"),
            ))
            .expect("test state should open");
        for (label, provider) in [
            ("openai-label", Provider::Openai),
            ("claude-label", Provider::Claude),
        ] {
            let account = AccountRecord::new(
                provider,
                account_id_from_label(label).expect("test account id should parse"),
                label,
                AccountStatus::Enabled,
            );
            runtime
                .block_on(state.upsert_account(&account))
                .expect("test account should persist");
        }
        runtime
            .block_on(state.close())
            .expect("test state should close");

        let mut output = Vec::new();
        list_accounts(&mut output, temporary_root.path().to_path_buf())
            .expect("account list should render");
        let output = String::from_utf8(output).expect("account list output should be UTF-8");
        assert!(output.contains("provider"));
        assert!(output.contains("openai"));
        assert!(output.contains("claude"));
    }
}

#[cfg(test)]
mod claude_login_glue_tests {
    use super::*;
    use codex_router_core::redaction::SecretString;
    use codex_router_secret_store::credential_bundle::CredentialBundle;
    use std::io::Cursor;
    use std::sync::Mutex;

    struct RecordingClaudeLoginFlow {
        flow: ClaudeOAuthLoginFlow,
        pasted_callbacks: Mutex<Vec<String>>,
    }

    impl RecordingClaudeLoginFlow {
        fn new() -> Self {
            Self {
                flow: ClaudeOAuthLoginFlow::new(),
                pasted_callbacks: Mutex::new(Vec::new()),
            }
        }
    }

    impl AccountLoginFlow for RecordingClaudeLoginFlow {
        type PendingLogin = PendingClaudeOAuthLogin;

        fn begin_login(&self) -> Result<Self::PendingLogin, LoginFlowError> {
            self.flow.begin_login()
        }

        fn authorization_url(
            &self,
            pending: &Self::PendingLogin,
        ) -> Result<String, LoginFlowError> {
            self.flow.authorization_url(pending)
        }

        fn finish_login(
            &self,
            _pending: Self::PendingLogin,
            pasted_callback: &str,
        ) -> Result<CredentialBundle, LoginFlowError> {
            self.pasted_callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(pasted_callback.to_owned());
            if pasted_callback.trim().is_empty() {
                return Err(LoginFlowError::InvalidCallback);
            }
            CredentialBundle::new_claude(
                SecretString::new("glue-access-canary"),
                SecretString::new("glue-refresh-canary"),
                2_000,
            )
            .map_err(|_| LoginFlowError::InvalidTokenResponse)
        }
    }

    #[test]
    fn claude_login_glue_prints_prompt_and_passes_the_pasted_callback() {
        let flow = RecordingClaudeLoginFlow::new();
        let mut stdout = Vec::new();
        let mut reader = Cursor::new(b"code-value#state-value\n".to_vec());

        let bundle = collect_claude_oauth_bundle(&mut stdout, &mut reader, &flow)
            .unwrap_or_else(|error| panic!("test login should finish: {error}"));

        let prompt = String::from_utf8(stdout)
            .unwrap_or_else(|error| panic!("test prompt should be UTF-8: {error}"));
        assert!(prompt.contains("Open this URL and finish the Claude authorization:"));
        assert!(prompt.contains("Paste the returned code#state: "));
        assert_eq!(
            *flow
                .pasted_callbacks
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            ["code-value#state-value\n"]
        );
        assert_eq!(bundle.access_token().expose_secret(), "glue-access-canary");
    }

    #[test]
    fn claude_login_glue_reports_eof_as_an_invalid_callback() {
        let flow = RecordingClaudeLoginFlow::new();
        let mut stdout = Vec::new();
        let mut reader = Cursor::new(Vec::<u8>::new());

        let error = collect_claude_oauth_bundle(&mut stdout, &mut reader, &flow)
            .expect_err("EOF must not activate an empty callback");

        assert!(matches!(
            error,
            AccountCommandError::ClaudeOAuth(LoginFlowError::InvalidCallback)
        ));
    }

    #[test]
    fn claude_login_glue_maps_provider_mismatch_from_activation() {
        assert!(matches!(
            map_credential_activation_error(CredentialActivationError::AccountProviderMismatch),
            AccountCommandError::AccountProviderMismatch
        ));
    }

    #[test]
    fn claude_login_reauthenticates_account_with_non_derived_id() {
        // Arrange: imported identity survives a different current display label.
        let root = tempfile::tempdir().unwrap();
        let runtime = account_command_runtime().unwrap();
        let state = runtime
            .block_on(AsyncSqliteStateStore::open(
                &root.path().join("state.sqlite"),
            ))
            .unwrap();
        let stored_id = AccountId::new("acct_imported_claude_identity").unwrap();
        let account = AccountRecord::new(
            Provider::Claude,
            stored_id.clone(),
            "current label",
            AccountStatus::Disabled,
        )
        .with_active_credential_generation(1);
        runtime.block_on(state.upsert_account(&account)).unwrap();
        runtime.block_on(state.close()).unwrap();
        let secrets = codex_router_secret_store::test_support::open_encrypted_credential_store(
            root.path().join("secrets"),
        )
        .unwrap();
        let flow = RecordingClaudeLoginFlow::new();
        let mut output = Vec::new();

        // Act: fake flow returns test credentials, with no OAuth network or Keychain.
        login_with_claude_oauth(
            &mut output,
            &mut Cursor::new(b"test-code#test-state\n"),
            root.path().to_path_buf(),
            "current label".to_owned(),
            &flow,
            Some(secrets.clone().into()),
        )
        .unwrap();

        // Assert: same account, next generation, newly activated fixture credentials.
        assert_eq!(flow.pasted_callbacks.lock().unwrap().len(), 1);
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("account_id: acct_imported_claude_identity")
        );
        let reopened = runtime
            .block_on(AsyncSqliteStateStore::open(
                &root.path().join("state.sqlite"),
            ))
            .unwrap();
        let accounts = runtime.block_on(reopened.list_accounts()).unwrap();
        assert_eq!(accounts.len(), 1);
        let active = runtime
            .block_on(reopened.load_account(&stored_id))
            .unwrap()
            .unwrap();
        assert_eq!(active.status(), AccountStatus::Enabled);
        assert_eq!(active.active_credential_generation(), Some(2));
        let key = codex_router_secret_store::account_tokens::provider_credential_bundle_key(
            Provider::Claude,
            &stored_id,
            2,
        )
        .unwrap();
        let bundle = CredentialBundle::from_secret_string(
            Provider::Claude,
            secrets.read_secret(&key).unwrap(),
        )
        .unwrap();
        assert_eq!(bundle.access_token().expose_secret(), "glue-access-canary");
        runtime.block_on(reopened.close()).unwrap();
    }
}
