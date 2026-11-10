use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::Duration;

use codex_router_core::ids::AccountId;
use codex_router_core::redaction::SecretString;
use codex_router_secret_store::SecretStore;
use codex_router_secret_store::account_tokens::AccountCredentialBundle;
use codex_router_secret_store::account_tokens::openai_account_credential_bundle_key;
use codex_router_secret_store::account_tokens::upstream_access_token_key;
use codex_router_secret_store::credential_bundle::CredentialBundle;
use codex_router_secret_store::encrypted_credential_store::EncryptedCredentialStore;
use codex_router_secret_store::keychain_data_key::KeychainAccess;
use codex_router_secret_store::keychain_data_key::KeychainAccessError;
use codex_router_secret_store::keychain_data_key::ROUTER_KEYCHAIN_SERVICE;
use codex_router_secret_store::model::SecretKey;
use codex_router_secret_store::model::SecretStoreError;
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::credential_maintenance::CredentialMaintenanceRecord;
use codex_router_state::credential_maintenance::CredentialMaintenanceState;
use codex_router_state::sqlite::AsyncSqliteStateStore;

use super::package_name;
use crate::oauth::OAuthRefreshClassification;
use crate::oauth::OAuthTokenStatus;
use crate::oauth::TokenClock;
use crate::oauth::classify_refresh_response;
use crate::quota_client::AuthenticatedQuotaClient;
use crate::quota_client::AuthenticatedQuotaError;
use crate::quota_client::QuotaFetchRequest;
use crate::quota_client::QuotaFetchResponse;
use crate::refresh_worker::AccountRefreshInput;
use crate::refresh_worker::RefreshWorkDecision;
use crate::refresh_worker::RefreshWorker;
use crate::resolver::AsyncRefreshLeaseRegistry;
use crate::resolver::AsyncRouterCredentialResolver;
use crate::resolver::CredentialRefreshClient;
use crate::resolver::CredentialResolverError;
use crate::resolver::NoopCredentialRefreshClient;
use crate::resolver::OpenAiOAuthRefreshClient;
use crate::resolver::credential_renewal_is_due;
use crate::router_credentials::RouterCredentialBundle;

#[path = "tests/credential_activation_tests.rs"]
mod credential_activation_tests;
#[path = "tests/credential_maintenance_reauth_tests.rs"]
mod credential_maintenance_reauth_tests;
#[path = "tests/credential_provider_mismatch_tests.rs"]
mod credential_provider_mismatch_tests;
#[path = "tests/credential_renewal_http_outcome_tests.rs"]
mod credential_renewal_http_outcome_tests;
#[path = "tests/credential_renewal_outcome_tests.rs"]
mod credential_renewal_outcome_tests;
#[path = "tests/credential_renewal_tests.rs"]
mod credential_renewal_tests;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[test]
fn reports_package_name() {
    assert_eq!(package_name(), "codex-router-auth");
}

#[test]
fn token_status_classifies_refresh_window_and_expiry() {
    let clock = TokenClock::new(1_000);

    assert_eq!(
        clock.classify_token(1_400, 120),
        OAuthTokenStatus::Valid {
            refresh_after_unix_seconds: 1_280
        }
    );
    assert_eq!(
        clock.classify_token(1_060, 120),
        OAuthTokenStatus::RefreshNeeded
    );
    assert_eq!(clock.classify_token(999, 120), OAuthTokenStatus::Expired);
}

#[test]
fn proactive_renewal_uses_short_lifetime_and_four_hour_age_boundaries() {
    let maintenance = CredentialMaintenanceRecord {
        credential_generation: 1,
        state: CredentialMaintenanceState::Healthy,
        failure_class: None,
        last_success_unix_seconds: Some(1_000),
        next_attempt_unix_seconds: None,
        claimed_successor_generation: None,
        claim_purpose: None,
        claim_started_unix_seconds: None,
        claim_prior_state: None,
        consecutive_failures: 0,
    };
    let short = CredentialBundle::from(
        AccountCredentialBundle::imported_codex_auth("short", Some("refresh".to_owned()))
            .with_expires_unix_seconds(1_120),
    );
    assert!(!credential_renewal_is_due(
        &short,
        Some(&maintenance),
        1_059
    ));
    assert!(credential_renewal_is_due(&short, Some(&maintenance), 1_060));
    let long = CredentialBundle::from(
        AccountCredentialBundle::imported_codex_auth("long", Some("refresh".to_owned()))
            .with_expires_unix_seconds(50_000),
    );
    assert!(!credential_renewal_is_due(
        &long,
        Some(&maintenance),
        15_399
    ));
    assert!(credential_renewal_is_due(&long, Some(&maintenance), 15_400));
    let unknown = CredentialBundle::from(AccountCredentialBundle::imported_codex_auth(
        "unknown",
        Some("refresh".to_owned()),
    ));
    assert!(!credential_renewal_is_due(
        &unknown,
        Some(&maintenance),
        15_399
    ));
    assert!(credential_renewal_is_due(
        &unknown,
        Some(&maintenance),
        15_400
    ));
    assert!(credential_renewal_is_due(&long, None, 1_000));
}

#[test]
fn refresh_response_classification_is_openai_oauth_specific() {
    assert_eq!(
        classify_refresh_response(200, None),
        OAuthRefreshClassification::Succeeded
    );
    assert_eq!(
        classify_refresh_response(400, Some("invalid_grant")),
        OAuthRefreshClassification::RefreshTokenRejected
    );
    assert_eq!(
        classify_refresh_response(429, None),
        OAuthRefreshClassification::RateLimited
    );
    assert_eq!(
        classify_refresh_response(503, None),
        OAuthRefreshClassification::TransientProviderFailure
    );
    assert_eq!(
        classify_refresh_response(418, Some("teapot")),
        OAuthRefreshClassification::UnexpectedProviderResponse { status: 418 }
    );
}

#[test]
fn openai_oauth_refresh_client_posts_codex_compatible_refresh_request() {
    let listener = must_ok(TcpListener::bind("127.0.0.1:0"));
    let server_address = must_ok(listener.local_addr());
    let server_thread = thread::spawn(move || {
        let (mut stream, _peer_address) = must_ok(listener.accept());
        let mut buffer = [0_u8; 4096];
        let bytes_read = must_ok(stream.read(&mut buffer));
        let request = String::from_utf8_lossy(&buffer[..bytes_read]);
        assert!(request.contains("POST /oauth/token HTTP/1.1"));
        assert!(request.contains("content-type: application/json"));
        assert!(request.contains(
                r#""client_id":"test-client","grant_type":"refresh_token","refresh_token":"refresh-token-canary""#
            ));
        must_ok(stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 73\r\n\r\n{\"access_token\":\"new-access-token\",\"expires_in\":60,\"token_type\":\"Bearer\"}",
            ));
    });
    let client = OpenAiOAuthRefreshClient::new_with_endpoint(
        format!("http://{server_address}/oauth/token"),
        "test-client",
    );

    let refreshed = must_ok(client.refresh_credentials(
        &account_id("acct_refresh_http"),
        &SecretString::new("refresh-token-canary"),
    ));

    assert_eq!(refreshed.access_token().expose_secret(), "new-access-token");
    assert_eq!(
        refreshed.refresh_token().map(SecretString::expose_secret),
        Some("refresh-token-canary")
    );
    assert!(refreshed.expires_unix_seconds().is_some());
    match server_thread.join() {
        Ok(()) => {}
        Err(error) => panic!("mock refresh server panicked: {error:?}"),
    }
}

#[test]
fn refresh_worker_selects_only_accounts_that_need_background_refresh() {
    let worker = RefreshWorker::new(TokenClock::new(1_000), 120);
    let decisions = worker.plan_refreshes(&[
        AccountRefreshInput::new("acct_valid", 1_400),
        AccountRefreshInput::new("acct_refresh", 1_050),
        AccountRefreshInput::new("acct_expired", 900),
    ]);

    assert_eq!(
        decisions,
        vec![
            RefreshWorkDecision::Skip {
                account_label: "acct_valid".to_owned(),
                token_status: OAuthTokenStatus::Valid {
                    refresh_after_unix_seconds: 1_280
                }
            },
            RefreshWorkDecision::Refresh {
                account_label: "acct_refresh".to_owned(),
                token_status: OAuthTokenStatus::RefreshNeeded
            },
            RefreshWorkDecision::Refresh {
                account_label: "acct_expired".to_owned(),
                token_status: OAuthTokenStatus::Expired
            }
        ]
    );
}

#[test]
fn authenticated_quota_client_contract_maps_mock_quota_response() {
    let client = FakeAuthenticatedQuotaClient;
    let response = match client.fetch_quota(QuotaFetchRequest::new("acct_primary", "responses")) {
        Ok(response) => response,
        Err(error) => panic!("quota fetch should succeed: {error}"),
    };

    assert_eq!(response.remaining_headroom(), 77);
    assert_eq!(response.route_name(), "responses");
}

#[path = "credential_publication_test.rs"]
mod credential_publication_tests;
#[path = "credential_resolver_test.rs"]
mod credential_resolver_tests;

struct FakeAuthenticatedQuotaClient;

impl AuthenticatedQuotaClient for FakeAuthenticatedQuotaClient {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    fn fetch_quota(
        &self,
        request: QuotaFetchRequest,
    ) -> Result<QuotaFetchResponse, AuthenticatedQuotaError> {
        assert_eq!(request.account_label(), "acct_primary");
        assert_eq!(request.route_name(), "responses");
        Ok(QuotaFetchResponse::new(request.route_name(), 77))
    }
}

#[derive(Clone)]
struct RefreshCompletionGate {
    entered_sender: std::sync::mpsc::Sender<()>,
    release_receiver: Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
}

#[derive(Clone)]
struct RecordingRefreshClient {
    expected_account_id: String,
    expected_refresh_token: String,
    response: AccountCredentialBundle,
    calls: Arc<AtomicUsize>,
    delay_millis: u64,
    completion_gate: Option<RefreshCompletionGate>,
}

impl RecordingRefreshClient {
    fn new(response: AccountCredentialBundle) -> Self {
        Self::new_for_account("acct_refresh_bundle", "refresh-token-canary", response)
    }

    fn new_for_account(
        expected_account_id: &str,
        expected_refresh_token: &str,
        response: AccountCredentialBundle,
    ) -> Self {
        Self {
            expected_account_id: expected_account_id.to_owned(),
            expected_refresh_token: expected_refresh_token.to_owned(),
            response,
            calls: Arc::new(AtomicUsize::new(0)),
            delay_millis: 0,
            completion_gate: None,
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn with_completion_gate(
        mut self,
        entered_sender: std::sync::mpsc::Sender<()>,
        release_receiver: std::sync::mpsc::Receiver<()>,
    ) -> Self {
        self.completion_gate = Some(RefreshCompletionGate {
            entered_sender,
            release_receiver: Arc::new(Mutex::new(release_receiver)),
        });
        self
    }

    fn with_delay_millis(mut self, delay_millis: u64) -> Self {
        self.delay_millis = delay_millis;
        self
    }
}

impl CredentialRefreshClient for RecordingRefreshClient {
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Assertions must fail this test while fixture errors propagate through Result."
    )]
    fn refresh_credentials(
        &self,
        account_id: &AccountId,
        refresh_token: &SecretString,
    ) -> Result<AccountCredentialBundle, crate::resolver::CredentialRefreshFailure> {
        assert_eq!(account_id.as_str(), self.expected_account_id);
        assert_eq!(refresh_token.expose_secret(), self.expected_refresh_token);
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(gate) = &self.completion_gate {
            must_ok(gate.entered_sender.send(()));
            must_ok(must_ok(gate.release_receiver.lock()).recv_timeout(Duration::from_secs(5)));
        }
        if self.delay_millis != 0 {
            thread::sleep(std::time::Duration::from_millis(self.delay_millis));
        }
        Ok(self.response.clone())
    }
}

#[derive(Default)]
struct CountingTestKeychain {
    items: Mutex<HashMap<(String, String), Vec<u8>>>,
    read_count: AtomicUsize,
    add_count: AtomicUsize,
}

impl KeychainAccess for CountingTestKeychain {
    fn read_secret(
        &self,
        service: &str,
        account: &str,
    ) -> Result<Option<Vec<u8>>, KeychainAccessError> {
        if service != ROUTER_KEYCHAIN_SERVICE {
            return Err(KeychainAccessError::ServiceRejected);
        }
        self.read_count.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .items
            .lock()
            .map_err(|_| KeychainAccessError::Unavailable)?
            .get(&(service.to_owned(), account.to_owned()))
            .cloned())
    }

    fn add_secret(
        &self,
        service: &str,
        account: &str,
        secret: &[u8],
    ) -> Result<(), KeychainAccessError> {
        if service != ROUTER_KEYCHAIN_SERVICE {
            return Err(KeychainAccessError::ServiceRejected);
        }
        self.add_count.fetch_add(1, Ordering::SeqCst);
        let mut items = self
            .items
            .lock()
            .map_err(|_| KeychainAccessError::Unavailable)?;
        let item_key = (service.to_owned(), account.to_owned());
        if items.contains_key(&item_key) {
            return Err(KeychainAccessError::Unavailable);
        }
        items.insert(item_key, secret.to_vec());
        Ok(())
    }
}

#[test]
fn router_credentials_debug_redacts_secret_fields() {
    let bundle = RouterCredentialBundle::new(
        "acct_primary",
        "access-token-canary",
        Some("refresh-token-canary"),
        Some(2_000),
    );
    let debug = format!("{bundle:?}");

    assert!(debug.contains("acct_primary"));
    assert!(debug.contains("access_token"));
    assert!(debug.contains("refresh_token"));
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("access-token-canary"));
    assert!(!debug.contains("refresh-token-canary"));
}

fn must_ok<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("expected Ok, got error: {error}"),
    }
}

fn account_id(value: &str) -> AccountId {
    match AccountId::new(value) {
        Ok(account_id) => account_id,
        Err(error) => panic!("account id should parse: {error}"),
    }
}

struct AuthTestTempDir {
    path: PathBuf,
}

impl AuthTestTempDir {
    fn new(name: &str) -> Self {
        let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!(
            "codex-router-auth-{name}-{}-{unique}",
            std::process::id()
        ));
        if path.exists() {
            remove_dir_all(&path);
        }
        if let Err(error) = fs::create_dir(&path) {
            panic!(
                "failed to create test directory {}: {error}",
                path.display()
            );
        }

        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for AuthTestTempDir {
    fn drop(&mut self) {
        if self.path.exists() {
            remove_dir_all(&self.path);
        }
    }
}

fn remove_dir_all(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path) {
        panic!(
            "failed to remove test directory {}: {error}",
            path.display()
        );
    }
}
