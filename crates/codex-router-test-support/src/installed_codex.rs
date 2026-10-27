//! Installed Codex smoke harness.

#[cfg(test)]
mod floor_switch;
mod retry;

pub use hostile_and_state::run_hostile_no_token_smoke;
pub use retry::run_all_weekly_exhausted_terminal;
pub use retry::run_three_account_short_quota_reconnect;
pub use smoke_modes::{
    InstalledCodexSmokeReport, run_installed_codex_http_sse_mock_smoke,
    run_installed_codex_mock_smoke, run_installed_codex_quota_reconnect_websocket_mock_smoke,
    run_installed_codex_s8_overlap_quota_websocket_mock_smoke,
    run_installed_codex_three_websocket_mock_e2e, run_installed_codex_three_websocket_mock_soak,
    run_installed_codex_websocket_mock_smoke,
};

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Read;
use std::io::Write;
use std::net::Shutdown;
use std::net::TcpListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::ExitStatus;
use std::process::Output;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Barrier;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use codex_native_integration::CodexRouterProfile;
use codex_router_cli::CliContext;
use codex_router_cli::profile::CodexRouterProfileWriter;
use codex_router_cli::run_with_io_async;
use codex_router_cli::token::LocalRouterTokenService;
use codex_router_cli::token::Shell;
use codex_router_cli::token::export_token_assignment;
use codex_router_core::ids::AccountId;
use codex_router_core::redaction::SecretString;
use codex_router_secret_store::SecretStore;
use codex_router_secret_store::account_tokens::AccountCredentialBundle;
use codex_router_secret_store::account_tokens::openai_account_credential_bundle_key;
use codex_router_secret_store::account_tokens::upstream_access_token_key;
use codex_router_secret_store::encrypted_credential_store::EncryptedCredentialStore;
use codex_router_state::account::AccountRecord;
use codex_router_state::account::AccountStatus;
use codex_router_state::quota_snapshot::PersistedQuotaSnapshot;
use codex_router_state::quota_snapshot::PersistedSelectorQuotaWindow;
use codex_router_state::quota_snapshot::QuotaSnapshotSource;
use codex_router_state::quota_snapshot::SelectorQuotaWindowStatus;
use codex_router_state::repositories::AccountStateRepository;
use codex_router_state::repositories::QuotaSnapshotRepository;
use codex_router_state::repositories::SelectorQuotaRepository;
use codex_router_state::sqlite::SqliteStateStore;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;
use tungstenite::Message;
use tungstenite::WebSocket;
use tungstenite::accept_hdr;
use tungstenite::client::IntoClientRequest;
use tungstenite::connect;
use tungstenite::handshake::server::Request;
use tungstenite::handshake::server::Response;
use tungstenite::stream::MaybeTlsStream;

const SMOKE_EXPECTED_TEXT: &str = "codex-router smoke ok";
const SMOKE_PROMPT: &str = "Reply with exactly: codex-router smoke ok";
const SMOKE_TARGET_MODEL: &str = "gpt-5.4-mini";
const SMOKE_TARGET_MODEL_OVERRIDE: &str = "model=\"gpt-5.4-mini\"";
const CODEX_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const UPSTREAM_ACCEPT_TIMEOUT: Duration = Duration::from_secs(35);
const DEFAULT_SOAK_DURATION: Duration = Duration::from_secs(300);
const SOAK_COMMAND_TIMEOUT_SLACK: Duration = Duration::from_secs(90);
const SOAK_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const SOAK_PROOF_MARGIN: Duration = Duration::from_secs(1);
const QUICK_CONCURRENT_HOLD_DURATION: Duration = Duration::from_secs(2);
const ROUTER_REGISTRY_DRAIN_TIMEOUT: Duration = Duration::from_secs(15);
const RETAIN_SMOKE_ROOT_ENV: &str = "CODEX_ROUTER_RETAIN_SMOKE_ROOT";
type PressureHandles = Arc<Mutex<Vec<thread::JoinHandle<Result<(), String>>>>>;
const INSTALLED_SMOKE_RUNTIME_ROOT_MODE_ENV: &str =
    "CODEX_ROUTER_INSTALLED_SMOKE_RUNTIME_ROOT_MODE";
const INSTALLED_SMOKE_ROUTER_ROOT_ENV: &str = "CODEX_ROUTER_INSTALLED_SMOKE_ROUTER_ROOT";
const INSTALLED_SMOKE_CODEX_HOME_ENV: &str = "CODEX_ROUTER_INSTALLED_SMOKE_CODEX_HOME";
const INSTALLED_SMOKE_PROCESS_HOME_ENV: &str = "CODEX_ROUTER_INSTALLED_SMOKE_PROCESS_HOME";
const S8_RUN_ID_ENV: &str = "CODEX_ROUTER_S8_RUN_ID";
const QUOTA_RECONNECT_SQLITE_PRESSURE_HOLD: Duration = Duration::from_secs(15);
const QUOTA_RECONNECT_SQLITE_PRESSURE_READY_TIMEOUT: Duration = Duration::from_secs(5);
const QUOTA_RECONNECT_ROUTER_MAX_CONNECTIONS: usize = 2;

#[path = "installed_codex/concurrent_websocket.rs"]
mod concurrent_websocket;

#[path = "installed_codex/hostile_and_state.rs"]
mod hostile_and_state;

#[path = "installed_codex/http_probe.rs"]
mod http_probe;

#[path = "installed_codex/process_runtime.rs"]
mod process_runtime;

#[path = "installed_codex/quota_reconnect_upstream.rs"]
mod quota_reconnect_upstream;

#[path = "installed_codex/s8_provenance.rs"]
mod s8_provenance;

#[path = "installed_codex/smoke_contracts.rs"]
mod smoke_contracts;

#[path = "installed_codex/smoke_modes.rs"]
mod smoke_modes;

#[path = "installed_codex/smoke_runtime.rs"]
mod smoke_runtime;
#[cfg(test)]
#[path = "installed_codex/tests.rs"]
mod tests;

#[path = "installed_codex/transcript_observation.rs"]
mod transcript_observation;

#[path = "installed_codex/websocket_models.rs"]
mod websocket_models;

#[path = "installed_codex/websocket_upstream.rs"]
mod websocket_upstream;

use concurrent_websocket::{
    overlap_duration_ms, real_overlap_duration_ms, run_concurrent_mock_websocket_session,
    wait_for_concurrent_session_barrier, wake_mock_upstream_accept,
};
use hostile_and_state::{
    QUOTA_RECONNECT_FALLBACK, QUOTA_RECONNECT_PRIMARY,
    QUOTA_RECONNECT_PRIMARY_FOR_INITIAL_ADMISSION, SmokeQuotaStatus, SmokeSeed,
    seed_quota_reconnect_router_state, seed_router_state, seed_s8_overlap_quota_router_state,
    selected_account_from_status_json, smoke_account_label_from_upstream_token,
};
use http_probe::{
    MockHttpRequestResult, SmokeTempRoot, accept_with_deadline, account_id, command_output_text,
    http_sse_transcript_summary, is_non_prewarm_response_create_frame, is_prewarm_request_frame,
    join_result, looks_like_websocket_upgrade, output_status_text, parse_posix_token_assignment,
    record_http_sse_only_transcript, redacted_optional_command_text, reserve_loopback_port,
    respond_to_http_request, run_mock_websocket, run_no_connection_upstream,
    run_s8_overlap_quota_local_probe, send_hostile_no_token_websocket, smoke_prewarm_events,
    smoke_response_events, timestamp_millis, timestamp_seconds,
};
use process_runtime::{
    CodexChildEnvironment, CodexTransportMode, RouterProcessStartOptions, SmokeContractAssertion,
    assert_codex_visible_output, process_output_markers, redacted_process_output_preview,
    run_codex_exec, run_codex_exec_with_timeout, run_codex_exec_with_timeout_observed,
    start_router_process, start_router_process_with_options, workspace_root,
};
use quota_reconnect_upstream::run_concurrent_mock_upstream;
use s8_provenance::{
    assert_redacted_quota_reconnect_payload, assert_redacted_three_websocket_payload,
    assert_redacted_transcript_payload, current_git_head, first_frame_shape_summary,
    observe_router_socket_cleanup, response_create_frame_shape_summary,
    runtime_correlations_for_three_websocket, s8_smoke_provenance, s8_smoke_run_id,
    sanitized_artifact_path, sanitized_loopback_endpoint_text, sanitized_router_argv,
    session_continuity_for_three_websocket, stderr_transport_error_markers,
};
use smoke_contracts::{
    assert_codex_quota_reconnect_output_is_safe, assert_quota_reconnect_contract,
    assert_smoke_contract, authorization_header_matches_expected, bearer_token_from_headers,
    http_sse_request_asks_streaming, quota_reconnect_label_from_upstream_token,
    quota_reconnect_role_from_label, quota_reconnect_usage_limit_frame,
    upstream_label_from_authorization_header,
};
use smoke_modes::{
    CodexChildRun, CodexExecRequest, ConcurrentUpstreamConfig, ConcurrentWebSocketHarnessConfig,
    InstalledCodexRuntimeRoots, InstalledCodexSmokeMode, UpstreamClientSessionObservation,
};
use smoke_runtime::{
    installed_codex_runtime_roots, run_installed_codex_three_websocket_mock_e2e_inner,
};
use transcript_observation::{
    QuotaReconnectTranscriptInput, RedactedTranscriptInput, RouterAuditObservation,
    RouterProcessObservation, RouterWebSocketRegistryReport, ThreeWebSocketTranscriptInput,
    write_redacted_quota_reconnect_transcript, write_redacted_three_websocket_transcript,
    write_redacted_transcript,
};
use websocket_models::{
    ConcurrentUpstreamSharedState, ConcurrentUpstreamState, ConcurrentWebSocketTranscript,
    MockConcurrentWebSocketUpstream, MockHttpSseTranscript, MockNoConnectionUpstream,
    MockQuotaReconnectWebSocketUpstream, MockWebSocketTranscript, MockWebSocketUpstream,
    QuotaReconnectSqlitePressureConfig, QuotaReconnectUpstreamState,
    QuotaReconnectWebSocketTranscript, RouterProcessGuard, RouterSocketCleanupObservation,
    S8OverlapQuotaErrorContext,
};
use websocket_upstream::{
    run_mock_upstream, run_quota_reconnect_mock_upstream, start_s8_overlap_quota_sqlite_pressure,
};
