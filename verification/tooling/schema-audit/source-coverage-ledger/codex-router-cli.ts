import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 67 colocated unit-test files (*_test.rs, *_tests.rs).

export const section: LedgerSection = {
  root: "crates/codex-router-cli",
  files: [
    {
      file: "crates/codex-router-cli/src/account.rs",
      disposition: { kind: "none" },
      note: "Account command parsing (121-209), OpenAI device and Claude OAuth login through codex-router-auth (492-656), SQLite account listing and status/floor mutations (742-900); OAuth and state-store calls only, no message shapes of the four protocols.",
    },
    {
      file: "crates/codex-router-cli/src/account_help.rs",
      disposition: { kind: "none" },
      note: "Account help text constants only (3-79).",
    },
    {
      file: "crates/codex-router-cli/src/account_login_identity.rs",
      disposition: { kind: "none" },
      note: "Resolves a login label to a stored AccountId (7-53) and derives acct_<stem> ids from labels (64-84); account identity, not a protocol identity.",
    },
    {
      file: "crates/codex-router-cli/src/account_options.rs",
      disposition: { kind: "none" },
      note: "Hand-written account option parsers (14-228); CLI arguments, no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/account_removal.rs",
      disposition: { kind: "none" },
      note: "Disabled-account removal under the credential lock and SQLite writer (14-121); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/central_command/arguments.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: clap arguments for `codex-router central serve|mint` (CentralCommand 5, ServeArguments 48, MintArguments 70, CredentialKind 65); configures the self-hosted central process, defines no message on the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/central_command/configuration.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: CentralConfiguration (Serialize/Deserialize, 16-22) is the on-disk central.json deployment file (AccessPolicy, fleet_sync_protocol ReceiverId and PeerId), written at 150 and read at 163; local configuration of the central server, not a message on the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/central_command/mod.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: starts collaboration_central_tokio::TokioServer (64-87) and mints AccessCaller assertions into CentralCredential secrets (98-155); central access and credential vocabulary owned by collaboration-central-core, not one of the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/cli_argument_parsing.rs",
      disposition: { kind: "none" },
      note: "Top-level command parser (CliCommand 19, ArgumentParser 310) and token/profile option parsing; CLI arguments, no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/cli_argument_parsing/serve_command.rs",
      disposition: { kind: "none" },
      note: "Serve option parsing and defaults (ServeCommand 20, ServeCommandOptions 123); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/cli_command_errors.rs",
      disposition: { kind: "none" },
      note: "CliError thiserror enum (16-211); WebSocketRegistryReportRender (193) only wraps a serde_json::Error, no JSON shape is defined here.",
    },
    {
      file: "crates/codex-router-cli/src/cli_contract_tests/cli_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at cli_contract_tests.rs:106 (#[path] 105), inside cli_contract_tests which lib.rs:855 declares under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-cli/src/cli_contract_tests/quota_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at cli_contract_tests.rs:110 (#[path] 109), inside cli_contract_tests which lib.rs:855 declares under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-cli/src/cli_contract_tests/transport_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at cli_contract_tests.rs:114 (#[path] 113), inside cli_contract_tests which lib.rs:855 declares under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-cli/src/credential_runtime.rs",
      disposition: { kind: "none" },
      note: "Credential resolver adapters over codex-router-auth (CliCredentialResolver 41, AsyncCliCredentialResolver 126); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/credential_upkeep_worker.rs",
      disposition: { kind: "none" },
      note: "OAuth upkeep worker thread; its sends (52, 74, 94; 74 and 94 cfg(test)) are the in-process WorkerControl mpsc channel (64), not a protocol edge.",
    },
    {
      file: "crates/codex-router-cli/src/credential_upkeep_worker/telemetry.rs",
      disposition: { kind: "none" },
      note: "Refresh-client wrapper recording OpenTelemetry outcome counters (22-96); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/credential_upkeep_worker/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at credential_upkeep_worker.rs:422 under #[cfg(test)] at 421",
      },
    },
    {
      file: "crates/codex-router-cli/src/doctor.rs",
      disposition: { kind: "none" },
      note: "Redacted human doctor report rendering (DoctorReport 42, render 55); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/fleet_command/enrollment_credentials.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: decodes the CentralCredential::Enrollment handoff (38-57) and builds Cloudflare Access / self-hosted headers (67-119) for the central admin request; central access credentials, not one of the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/fleet_command/enrollment_http.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: hand-repeats the central admin envelope EnrollmentBody {kind,value} (7-11, tag/content serde) that collaboration-central-core central_router.rs:157 defines, POSTs it to /admin/v1/machines/enroll (26-30) and decodes EnrollmentDecision (53); central admin protocol, not one of the six edges or four protocols",
      },
      note: "Out-of-scope duplicate: the Serialize-side copy of central_router.rs:150-159 EnrollmentBody lives here by hand.",
    },
    {
      file: "crates/codex-router-cli/src/fleet_command/enrollment_prepare.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: builds collaboration_central_core::EnrollMachine (99-106) and the /admin/v1/machines/enroll URL (59) from CLI inputs and local installation identity; central admin request, not one of the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/fleet_command/enrollment_publish.rs",
      disposition: {
        kind: "excluded",
        reason:
          "central-only: maps EnrollmentDecision::MachineEnrolled (21-31) into the local FleetEnrollmentDocument (68-87) and staged secrets; central enrollment state, not one of the six edges or four protocols",
      },
    },
    {
      file: "crates/codex-router-cli/src/host_command/foreground_launch.rs",
      disposition: { kind: "none" },
      note: "Foreground Host composition: app-server ChildCommandSpec and HostConfig (59-206), replacement argv (274-336); process and configuration wiring, no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/foreground_launch/debug_profile_loader.rs",
      disposition: { kind: "none" },
      note: "Loads codex-router-debug.config.toml through DebugCodexProfile (9-25); Codex configuration, not an app-server message.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/foreground_launch/router_child_command.rs",
      disposition: { kind: "none" },
      note: "Router child `serve` argv and OTLP environment (22-69); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/host_stop.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "Writes encode_operator_request(OperatorRequest::StopHost) directly on the Host-CLI operator Unix socket (84-87) after peer-credential checks, bypassing operator_client and reading no reply. The operator socket is not one of the six outside edges, so no sendSites row.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/mod.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "Maps HostAction to OperatorRequest (276-291) and sends it through operator_client::send_operator_request_streaming (294-299); classifies the terminal frame's TerminalClassification Failed|Busy|Unavailable as command failure (347-359). Operator socket, not one of the six outside edges.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/operator_client.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "Writes encode_operator_request bytes to the Host-CLI operator Unix socket (154-158) and decodes newline-delimited OperatorFrame replies with decode_operator_frame (168-194); HostProgress predicates (71-99). Codecs come from codex-router-host operator_messages.rs. The operator socket is not one of the six outside edges.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/process_exit_observer.rs",
      disposition: { kind: "none" },
      note: "kqueue/pidfd process-exit wait (6-101); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/provider_launch_configuration.rs",
      disposition: { kind: "none" },
      note: "Merges settings.jsonc provider entries with explicit flags into ExternalProviderStartup (10-86); launch configuration, not an ACP message.",
    },
    {
      file: "crates/codex-router-cli/src/host_command/replacement_outcome.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: 'Classifies operator frame sequences into UpdateResult (56-125, 127-161) and HostRestartResult (212-256), including a string match on the terminal message "app-server restarted" (79); sends OperatorRequest::AwaitHostStart through operator_client (262-266, 291-296) and maps Ready|LocalReadyRemoteDegraded to readiness (269-280, 299-310). Operator socket, not one of the six outside edges.',
    },
    {
      file: "crates/codex-router-cli/src/lib.rs",
      disposition: { kind: "none" },
      note: "Command dispatch, serve composition and CliContext. Serve hands Codex traffic to codex-router-proxy LoopbackRouterRuntime::serve_protocol_connections (321); every wire write happens in the proxy crate. Renders HostProgress::RouterReady to stdout through presentation::host (302-305); no message shape defined or converted here.",
    },
    {
      file: "crates/codex-router-cli/src/live.rs",
      disposition: { kind: "none" },
      note: "Live quota diagnostic: LiveQuotaClient::fetch_from_auth_json (172) reads the ChatGPT backend usage endpoint and auth.json, both decoded in codex-router-auth; the ChatGPT usage API is not one of the six outside edges. LiveQuotaError to label strings (277-286) is human stdout only.",
    },
    {
      file: "crates/codex-router-cli/src/main.rs",
      disposition: { kind: "none" },
      note: "Binary entry point: calls run_async (3).",
    },
    {
      file: "crates/codex-router-cli/src/presentation/host.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (Host operator socket)",
      },
      note: "Renders Host-CLI operator socket shapes (OperatorFrame, HostTerminalResponse, HostSnapshot, HostProgress, UpdateResult from codex-router-host operator_messages.rs / lifecycle_state.rs) as human text. Display projection of TerminalClassification in classification_label (418-432, HostedReadiness picks the readiness label) and of HostProgress in accept (32-36, 45-49); the Host operator socket is outside Project 2's protocols, so no inventory row. serde_json::to_string of the central-link FleetLinkFailure reason (359) is embedded in human stdout only; central link is not one of the four protocols.",
    },
    {
      file: "crates/codex-router-cli/src/presentation/mod.rs",
      disposition: { kind: "none" },
      note: "iocraft presentation boundary marker (TerminalPresentationLayer 12); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/profile.rs",
      disposition: { kind: "none" },
      note: "Writes the Codex profile TOML (codex-router.config.toml, 96) rendered by codex-native-integration CodexRouterProfile; Codex configuration file, not an app-server message.",
    },
    {
      file: "crates/codex-router-cli/src/profile_preview.rs",
      disposition: { kind: "none" },
      note: "Profile dry-run preview with secret-key redaction (SECRET_KEYS 31); text output only.",
    },
    {
      file: "crates/codex-router-cli/src/quota.rs",
      disposition: { kind: "none" },
      note: "Quota module wiring and re-exports; persisted quota state, no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_account_adapter.rs",
      disposition: { kind: "none" },
      note: "Quota TUI account actions (copy id, enable, disable, remove) against the router state store; StateStoreError/AccountRemovalError to QuotaAccountRefusal mapping (158-173) is local UI vocabulary, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_account_factory_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          'cfg(test)-only module: declared at quota/quota_account_adapter.rs:184 under #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]',
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_account_run_session.rs",
      disposition: { kind: "none" },
      note: "Runs the quota account command queue beside the render loop; in-process only.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_background_composition.rs",
      disposition: { kind: "none" },
      note: "Composes the background quota refresh worker (codex-router-quota-service); provider usage refresh, not one of the six outside edges.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_command_dispatch.rs",
      disposition: { kind: "none" },
      note: "Quota argument parsing, QuotaCommandError and dispatch; JSON output format delegates to quota-service write_quota_json; no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_command_options.rs",
      disposition: { kind: "none" },
      note: "Quota option parsing (table|plain|json format, 72-81); no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_credential_resources.rs",
      disposition: { kind: "none" },
      note: "Opens the CLI secret store and loads the quota status report; no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/configuration.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:2, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/control_server.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:3, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/effect_ports.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:4, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/event_fence.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:5, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/grouping_reopen.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:6, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/mod.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at quota/quota_account_adapter_test.rs:175 (cfg at :173), itself the test module declared at quota/quota_account_adapter.rs:182 under #[cfg(test)] (:180)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/owned_state.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:8, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/receipts.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:9, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/refresh_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:11, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_effectful_fixture/validation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared in quota/quota_effectful_fixture/mod.rs:14, itself a test module (declared in test module quota/quota_account_adapter_test.rs:175)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_group_preferences.rs",
      disposition: { kind: "none" },
      note: "Quota group-by preference in the local UI database (agent-proxy-ui.sqlite, key quota/group-by, 34, 45); local UI state, no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_login_child.rs",
      disposition: { kind: "none" },
      note: "Spawns a codex-router account login child process (34-43) and maps its exit status (78-92); process control only.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_login_wait_observation_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          'cfg(test)-only module: declared at quota/quota_login_child.rs:150 under #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]',
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_preference_write_observation_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          'cfg(test)-only module: declared at quota/quota_group_preferences.rs:60 under #[cfg(all(test, debug_assertions, feature = "quota-reset-test-harness"))]',
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_refresh_adapter.rs",
      disposition: { kind: "none" },
      note: "Maps the quota-service refresh report to the quota TUI QuotaRefreshResult (93-141); provider usage refresh, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_refresh_command.rs",
      disposition: { kind: "none" },
      note: "Quota refresh command and allowed ChatGPT base URL check (38-43); provider usage refresh, not one of the six outside edges.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_refresh_observation_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at quota/quota_refresh_adapter.rs:156 under #[cfg(test)] (:154)",
      },
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_status_command.rs",
      disposition: { kind: "none" },
      note: "Quota status rendering, interactive loop, credit-policy and weekly-floor savers; StateStoreError to TUI save-error mappings (234-248, 362-373) are local UI vocabulary; ResetSessionIntent::Shutdown (200) is an in-process channel send.",
    },
    {
      file: "crates/codex-router-cli/src/quota/quota_status_rendering.rs",
      disposition: { kind: "none" },
      note: "Static quota table renderer selection; terminal output only.",
    },
    {
      file: "crates/codex-router-cli/src/quota_reset.rs",
      disposition: { kind: "none" },
      note: "Declares the feature-gated (quota-reset-test-harness) reset harness module (2-5).",
    },
    {
      file: "crates/codex-router-cli/src/quota_reset/quota_reset_loopback_harness.rs",
      disposition: { kind: "none" },
      note: "Feature-gated (quota-reset-test-harness, quota_reset.rs:2) harness argument parser and composition against a loopback provider listener; not cfg(test), but carries no protocol shapes.",
    },
    {
      file: "crates/codex-router-cli/src/secret_store_factory.rs",
      disposition: { kind: "none" },
      note: "Opens the CLI runtime credential store; no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/telemetry.rs",
      disposition: { kind: "none" },
      note: "OTLP telemetry setup and log filter capping agent_client_protocol frame logging (181-183); the JSON at 520-525 is in the #[cfg(test)] module (388).",
    },
    {
      file: "crates/codex-router-cli/src/token.rs",
      disposition: { kind: "none" },
      note: "POSIX shell export of the local router token (15-32); no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/token_reload_watcher.rs",
      disposition: { kind: "none" },
      note: "Polls the local token generation and reloads proxy auth; no message shapes.",
    },
    {
      file: "crates/codex-router-cli/src/websocket_reporting.rs",
      disposition: { kind: "none" },
      note: "Hand-built json! WebSocket registry diagnostic file (schema_version 2, 32-52) written after serve; read only by tests (codex-router-cli tests/integration, codex-router-test-support installed_codex), not a message on any edge or between parts.",
    },
  ],
  sendSites: [],
};
