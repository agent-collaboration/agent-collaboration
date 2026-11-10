use super::*;

pub(super) async fn migrate_test_state_database(state_database_path: &Path) {
    let state = must_ok(AsyncSqliteStateStore::open(state_database_path).await);
    must_ok(state.close().await);
}

pub(super) struct TestRoot {
    pub(super) path: PathBuf,
}

impl TestRoot {
    pub(super) fn new(name: &str) -> Self {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!(
            "codex-router-cli-token-{name}-{}-{counter}",
            std::process::id()
        ));
        if path.exists() {
            remove_dir_all(&path);
        }

        Self { path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }
}

pub(super) fn default_router_root_for_test() -> PathBuf {
    let Some(home) = std::env::var_os("HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
        panic!("HOME must be set for default router root tests");
    };

    home.join(".codex-router")
}

pub(super) fn default_router_secret_root_for_test() -> PathBuf {
    default_router_root_for_test().join("secrets")
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        if self.path.exists() {
            remove_dir_all(&self.path);
        }
    }
}

pub(super) fn assert_router_profile_contract(output: &str, port: u16) {
    assert!(!output.contains("[profiles.codex-router]\n"));
    assert!(output.contains("model_provider = \"codex-router\"\n"));
    assert!(output.contains("[model_providers.codex-router]\n"));
    assert!(output.contains("name = \"OpenAI\"\n"));
    assert!(output.contains("enable_request_compression = false\n"));
    assert!(output.contains(format!("base_url = \"http://127.0.0.1:{port}/v1\"\n").as_str()));
    assert!(output.contains("wire_api = \"responses\"\n"));
    assert!(output.contains("requires_openai_auth = true\n"));
    assert!(output.contains("supports_websockets = true\n"));
    assert!(!output.contains("env_key"));
    assert!(!output.contains("env_http_headers"));
    assert!(!output.contains("sk-"));
    assert!(!output.contains("oauth"));
}

#[track_caller]
pub(super) fn must_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("expected Ok, got error: {error}"),
    }
}

pub(super) fn strip_ansi_sequences(input: &str) -> String {
    let mut output = String::new();
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\x1b' && characters.peek() == Some(&'[') {
            characters.next();
            for sequence_character in characters.by_ref() {
                if sequence_character.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

pub(super) fn lock_test_mutex<'a, T>(mutex: &'a Mutex<T>, label: &str) -> MutexGuard<'a, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(error) => panic!("{label} lock should be available: {error}"),
    }
}

pub(super) fn must_err<T, E: std::fmt::Display>(result: Result<T, E>) -> E {
    match result {
        Ok(_) => panic!("expected error, got Ok"),
        Err(error) => error,
    }
}

pub(super) fn remove_dir_all(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        panic!(
            "failed to remove test directory {}: {error}",
            path.display()
        );
    }
}

pub(super) struct CliRunOutput {
    pub(super) stdout: String,
    pub(super) stderr: String,
}

pub(super) fn run_cli_in_blocking_context(
    arguments: Vec<OsString>,
    context: CliContext,
) -> CliRunOutput {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    must_ok(run_with_io(arguments, &context, &mut stdout, &mut stderr));
    CliRunOutput {
        stdout: must_ok(String::from_utf8(stdout)),
        stderr: must_ok(String::from_utf8(stderr)),
    }
}
pub(super) async fn run_cli<const ARGUMENT_COUNT: usize>(
    args: [&str; ARGUMENT_COUNT],
    context: CliContext,
) -> CliRunOutput {
    let arguments = args.into_iter().map(Into::into).collect::<Vec<_>>();
    if matches!(
        must_ok(CliCommand::parse(arguments.clone())),
        CliCommand::Quota(_)
    ) {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        must_ok(run_with_io_async(arguments, &context, &mut stdout, &mut stderr).await);
        CliRunOutput {
            stdout: must_ok(String::from_utf8(stdout)),
            stderr: must_ok(String::from_utf8(stderr)),
        }
    } else {
        let command_dispatcher = tracing::dispatcher::get_default(Clone::clone);
        must_ok(
            tokio::task::spawn_blocking(move || {
                tracing::dispatcher::with_default(&command_dispatcher, || {
                    run_cli_in_blocking_context(arguments, context)
                })
            })
            .await,
        )
    }
}

pub(super) async fn ensure_async_state_schema(router_root: &Path) {
    let state = must_ok(
        AsyncSqliteStateStore::open(&codex_router_state::provider_proxy_database_path(
            router_root,
        ))
        .await,
    );
    must_ok(state.close().await);
}

pub(super) fn path_to_str(path: &Path) -> &str {
    match path.to_str() {
        Some(path) => path,
        None => panic!("test path must be UTF-8"),
    }
}

pub(super) fn account_id(value: &str) -> AccountId {
    match AccountId::new(value) {
        Ok(account_id) => account_id,
        Err(error) => panic!("account id should parse: {error}"),
    }
}
