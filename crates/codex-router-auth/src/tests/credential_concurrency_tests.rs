//! Real separate-instance/process rotations, synchronized at observed boundaries.
use super::*;
use codex_router_core::provider::Provider;
use std::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn independent_async_resolvers_use_one_rotating_refresh_generation() {
    let temp_dir = AuthTestTempDir::new("independent-async-refresh");
    let database_path = temp_dir.path().join("state.sqlite");
    let secret_path = temp_dir.path().join("secrets");
    let state = must_ok(AsyncSqliteStateStore::open(&database_path).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(&secret_path),
    );
    let account_id = account_id("independent-async-account");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "independent",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
    );
    let active_key = must_ok(openai_account_credential_bundle_key(&account_id, 1));
    must_ok(
        secrets.write_secret(
            &active_key,
            &must_ok(
                AccountCredentialBundle::imported_codex_auth(
                    "expired-access-canary",
                    Some("original-refresh-canary".to_owned()),
                )
                .with_expires_unix_seconds(900)
                .to_secret_string(),
            ),
        ),
    );
    let (provider_entered, observe_provider) = std::sync::mpsc::channel();
    let (release_provider, provider_release) = std::sync::mpsc::channel();
    let (second_read, mut observe_second_read) = tokio::sync::mpsc::unbounded_channel();
    let refresh_client = RecordingRefreshClient::new_for_account(
        "independent-async-account",
        "original-refresh-canary",
        AccountCredentialBundle::imported_codex_auth(
            "replacement-access-canary",
            Some("replacement-refresh-canary".to_owned()),
        )
        .with_expires_unix_seconds(2_000),
    )
    .with_response_gate(provider_entered, provider_release);
    let mut observe_provider = Some(observe_provider);
    let mut tasks = Vec::new();
    for resolver_index in 0..2 {
        if resolver_index == 1 {
            let entered = observe_provider.take().unwrap();
            must_ok(
                tokio::task::spawn_blocking(move || entered.recv_timeout(Duration::from_secs(2)))
                    .await,
            )
            .expect("first real resolver must hold its provider response");
        }
        let second_read = second_read.clone();
        let observed_secrets = ObservedSecretReadStore {
            inner: secrets.clone(),
            read_observer: Arc::new(move || {
                if resolver_index == 1 {
                    second_read
                        .send(())
                        .expect("second resolver secret read must be observed");
                }
            }),
        };
        let resolver = AsyncRouterCredentialResolver::new(
            must_ok(AsyncSqliteStateStore::open(&database_path).await),
            observed_secrets,
            refresh_client.clone(),
            Some(1_000),
        );
        let account_id = account_id.clone();
        tasks.push(tokio::spawn(async move {
            resolver
                .resolve_provider_credentials(&account_id, Provider::Openai)
                .await
        }));
    }
    tokio::time::timeout(Duration::from_secs(2), observe_second_read.recv())
        .await
        .unwrap()
        .expect("second resolver must read the old generation while first provider is held");
    must_ok(release_provider.send(()));
    for task in tasks {
        let credential =
            must_ok(must_ok(tokio::time::timeout(Duration::from_secs(2), task).await).unwrap());
        assert_eq!(credential.credential_generation(), 2);
    }
    assert_eq!(refresh_client.calls(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn separate_process_resolvers_use_one_rotating_refresh() {
    if let Some(root) = std::env::var_os("CODEX_ROUTER_CROSS_PROCESS_REFRESH_ROOT") {
        let root = PathBuf::from(root);
        let mut control = TcpStream::connect(
            std::env::var("CODEX_ROUTER_CROSS_PROCESS_REFRESH_CONTROL").unwrap(),
        )
        .unwrap();
        control
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        control
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let state = must_ok(AsyncSqliteStateStore::open(&root.join("state.sqlite")).await);
        let secrets = must_ok(
            codex_router_secret_store::test_support::open_encrypted_credential_store(
                root.join("secrets"),
            ),
        );
        control.write_all(b"R").unwrap();
        let mut start = [0];
        control.read_exact(&mut start).unwrap();
        assert_eq!(&start, b"S");
        let resolver = AsyncRouterCredentialResolver::new(
            state,
            secrets,
            CrossProcessRefreshClient {
                root,
                control: Some(Arc::new(Mutex::new(control))),
            },
            Some(1_000),
        );
        let resolved = must_ok(
            resolver
                .resolve_provider_credentials(
                    &account_id("cross-process-account"),
                    codex_router_core::provider::Provider::Openai,
                )
                .await,
        );
        assert_eq!(resolved.credential_generation(), 2);
        return;
    }

    let temp_dir = AuthTestTempDir::new("cross-process-refresh");
    let root = temp_dir.path();
    let state = must_ok(AsyncSqliteStateStore::open(&root.join("state.sqlite")).await);
    let secrets = must_ok(
        codex_router_secret_store::test_support::open_encrypted_credential_store(
            root.join("secrets"),
        ),
    );
    let account_id = account_id("cross-process-account");
    must_ok(
        state
            .upsert_account(
                &AccountRecord::new(
                    codex_router_core::provider::Provider::Openai,
                    account_id.clone(),
                    "cross process",
                    AccountStatus::Enabled,
                )
                .with_active_credential_generation(1),
            )
            .await,
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
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let control_address = listener.local_addr().unwrap();
    let mut child = must_ok(
        Command::new(std::env::current_exe().expect("test binary"))
            .arg("--exact")
            .arg("tests::credential_concurrency_tests::separate_process_resolvers_use_one_rotating_refresh")
            .env("CODEX_ROUTER_CROSS_PROCESS_REFRESH_ROOT", root)
            .env("CODEX_ROUTER_CROSS_PROCESS_REFRESH_CONTROL", control_address.to_string())
            .stdout(std::process::Stdio::null())
            .spawn(),
    );
    let mut control = tokio::time::timeout(Duration::from_secs(5), async {
        let (mut control, _) = listener.accept().await.unwrap();
        let mut ready = [0];
        control.read_exact(&mut ready).await.unwrap();
        assert_eq!(&ready, b"R");
        control.write_all(b"S").await.unwrap();
        control.read_exact(&mut ready).await.unwrap();
        assert_eq!(
            &ready, b"P",
            "child provider response must be held before parent resolution"
        );
        control
    })
    .await
    .expect("child must reach its real provider boundary");
    let (parent_read, mut observe_parent_read) = tokio::sync::mpsc::unbounded_channel();
    let secrets = ObservedSecretReadStore {
        inner: secrets,
        read_observer: Arc::new(move || {
            parent_read
                .send(())
                .expect("parent secret read must be observed");
        }),
    };
    let resolver = AsyncRouterCredentialResolver::new(
        state,
        secrets,
        CrossProcessRefreshClient {
            root: root.to_path_buf(),
            control: None,
        },
        Some(1_000),
    );
    let resolution = resolver.resolve_provider_credentials(&account_id, Provider::Openai);
    tokio::pin!(resolution);
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            result = &mut resolution => panic!("parent completed while child provider was held: {result:?}"),
            read = observe_parent_read.recv() => read.expect("parent must read old credentials before child release"),
        }
    }).await.expect("parent must enter real credential resolution");
    control.write_all(b"X").await.unwrap();
    let resolved = must_ok(
        tokio::time::timeout(Duration::from_secs(5), resolution)
            .await
            .unwrap(),
    );
    assert_eq!(resolved.credential_generation(), 2);
    assert!(
        must_ok(child.wait()).success(),
        "child resolver should finish"
    );
    let provider_calls = must_ok(fs::read_dir(root))
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("provider-call-")
        })
        .count();
    assert_eq!(provider_calls, 1);
}

#[derive(Clone)]
struct CrossProcessRefreshClient {
    root: PathBuf,
    control: Option<Arc<Mutex<TcpStream>>>,
}

impl CrossProcessRefreshClient {
    fn wait_for_response_release(&self) {
        if let Some(control) = &self.control {
            let mut control = control.lock().unwrap();
            control.write_all(b"P").unwrap();
            let mut release = [0];
            control.read_exact(&mut release).unwrap();
            assert_eq!(&release, b"X");
        }
    }
}

impl CredentialRefreshClient for CrossProcessRefreshClient {
    #[expect(
        clippy::unwrap_in_result,
        reason = "This test double must fail on broken fixture invariants instead of reporting a product error."
    )]
    fn refresh_credentials(
        &self,
        _account_id: &AccountId,
        _refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, crate::resolver::CredentialRefreshFailure> {
        fs::write(
            self.root
                .join(format!("provider-call-{}", std::process::id())),
            b"used",
        )
        .expect("provider call should be recorded");
        self.wait_for_response_release();
        Ok(AccountCredentialBundle::imported_codex_auth(
            "replacement-access-canary",
            Some("replacement-refresh-canary".to_owned()),
        )
        .with_expires_unix_seconds(2_000))
    }
}

#[derive(Clone)]
struct ObservedSecretReadStore {
    inner: EncryptedCredentialStore,
    read_observer: Arc<dyn Fn() + Send + Sync>,
}

impl SecretStore for ObservedSecretReadStore {
    fn write_secret(&self, key: &SecretKey, secret: &SecretString) -> Result<(), SecretStoreError> {
        self.inner.write_secret(key, secret)
    }

    fn read_secret(&self, key: &SecretKey) -> Result<SecretString, SecretStoreError> {
        let secret = self.inner.read_secret(key)?;
        (self.read_observer)();
        Ok(secret)
    }

    fn write_staged(&self, key: &SecretKey, secret: &SecretString) -> Result<(), SecretStoreError> {
        self.inner.write_staged(key, secret)
    }

    fn delete_staged(&self, key: &SecretKey) -> Result<(), SecretStoreError> {
        self.inner.delete_staged(key)
    }

    fn prune_obsolete_generations(
        &self,
        provider: Provider,
        account_id: &AccountId,
        previously_active_generation: u64,
    ) -> Result<Vec<u64>, SecretStoreError> {
        self.inner
            .prune_obsolete_generations(provider, account_id, previously_active_generation)
    }
}
