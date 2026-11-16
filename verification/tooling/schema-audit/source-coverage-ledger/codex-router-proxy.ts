import type { LedgerSection } from "../schema-audit-types.ts";
// Left out by name only: 71 colocated unit-test files (*_test.rs, *_tests.rs). Part 1 of 2 by path order (account_selection.rs to server/*); part 2 is codex-router-proxy-2.ts.

export const section: LedgerSection = {
  root: "crates/codex-router-proxy",
  files: [
    {
      file: "crates/codex-router-proxy/src/account_selection.rs",
      disposition: { kind: "none" },
      note: "Account-selection boundary types (ProviderRouteBand, SelectedAccountDecision, QuotaAwareAccountSelectorError L481, AccountDecisionSelector traits L450/L461) and module declarations; no serde, no JSON, no message shape. Test module declared #[cfg(test)] at L582-L584.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_admission.rs",
      disposition: { kind: "none" },
      note: "Live floor-switch peer and source-account admission over the burn-down assessment. The SelectorQuotaWindowStatus to QuotaWindowStatus match at L287 maps two in-process quota enums (state crate to selection crate), not a protocol vocabulary; no serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/affinity_race_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:51-52, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/affinity_write_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:54-55, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/lease_report_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:45-46, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/lease_wait_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:63-64, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/projection_read_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:48-49, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/quota_input_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:57-58, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/account_selection_tests/test_identity_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at account_selection/account_selection_tests.rs:60-61, a module itself declared #[cfg(test)] at account_selection.rs:582-584",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/active_reservations.rs",
      disposition: { kind: "none" },
      note: "In-process reservation book, RAII guard and SqliteActiveClientLeaseReporter enqueuing DbWriteCommand active_client_acquired/released (L61, L83) on the local DB write actor; telemetry hashes. No serde, no message shape on any edge.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/affinity_admission.rs",
      disposition: {
        kind: "excluded",
        reason:
          "outside Project 2's protocols (model proxy: OpenAI Responses and Anthropic Messages traffic)",
      },
      note: "session_id_for_route L9 reads the client session identity from model-traffic request headers: x-claude-code-session-id L14 for Claude Messages, session-id L16 for previous_response_id-capable Responses routes; empty values dropped. Otherwise affinity-owner and assessment helpers with no message shape. Headers of N2 model traffic, outside the four P2 protocols.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/assessment_selection.rs",
      disposition: { kind: "none" },
      note: "Burn-down assessment selection, holds and SnapshotFreshness to QuotaWindowStatus L347 (in-process quota enums); selection reason is RoutingReason::as_str for telemetry L355. No serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/fixture_selection.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at account_selection.rs:96-97 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/post_exhaustion.rs",
      disposition: { kind: "none" },
      note: "route_band_post_exhaustion_outcome L35 classifies post-exhaustion alternatives into PostExhaustionRouteBandOutcome from persisted projection; no serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/repository_selection.rs",
      disposition: { kind: "none" },
      note: "AsyncRepositoryBackedAccountSelector::select_upstream_account L246 composes helpers: previous_response_id(request) L311 (parsed in request_metadata.rs), hash_previous_response_id L326 into the stored affinity key, session_id_for_route L356 (affinity_admission.rs). It defines and parses no message shape itself.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/request_metadata.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "previous_response_id L50 reads the top-level Responses request field via the hand-written bounded scanner top_level_json_string_field L65 (64 KiB, at most 64 top-level keys, string values only, serde_json::from_slice::<String> unescape); route_kind_for_request L10 calls routes.rs classify_route; route_profile_for_kind L24 maps RouteKind to a selection RouteProfile (in-process). OpenAI Responses model traffic relayed by N2, outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/runtime_quarantine.rs",
      disposition: { kind: "none" },
      note: "Process-local runtime exhaustion and route-band queue health maps keyed by format! strings L34/L38; no serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/selection_diagnostics.rs",
      disposition: { kind: "none" },
      note: "Closed tracing labels: SelectionDiagnosticStage::as_str L28, StateStoreError and QuotaAwareAccountSelectorError to error.class strings L197/L240 for warn logs only; not written on any edge, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/account_selection/short_quota_wait.rs",
      disposition: { kind: "none" },
      note: "Short-quota wait delay and jitter arithmetic over BurnDownAccountInput windows; debug-only env read L27; no serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/attempt_loop.rs",
      disposition: { kind: "none" },
      note: "run_at_most_two L54 drives ClaudeAttemptPipeline::send L66/L91 (provider HTTP egress to Anthropic, not one of the six outside edges) and the injected classifier; is_account_rejection L114 matches AttemptOutcome. No serde, no message shape defined.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/attempt_loop/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/attempt_loop.rs:121-122 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/classifier.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: Anthropic Messages traffic)",
      },
      note: "classify L13 maps HTTP status, anthropic-ratelimit-unified-* headers (shared_rejected_window L61, reset_for_window L86, has_model_or_overage_rejection L95, has_explicit_quota_attribution L120) and the error body (serde_json Value L136, error L139, error.type L143, error.message L147) into AttemptOutcome/PassThroughReason. Anthropic Messages model traffic, outside the four P2 protocols.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/classifier/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/classifier.rs:167-168 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/content_encoding.rs",
      disposition: { kind: "none" },
      note: "HTTP content-coding chain parse of content-encoding L54 (gzip, deflate, br, zstd) and bounded decoders for inspection copies; HTTP transport layer, no JSON or message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/forward.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: Anthropic Messages traffic)",
      },
      note: "ClaudeEdge::prepare_upstream L74 re-checks the route path /anthropic/v1/messages L85 and strips /anthropic L94 (a second copy of the routes.rs:77 entry); prepare_headers L110 merges anthropic-beta tokens, appends oauth-2025-04-20 L135 and drops x-api-key L140. inspect_response L252 buffers up to 64 KiB of error body for the classifier. Body bytes otherwise pass through unparsed.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/forward/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/forward.rs:373-374 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/mod.rs",
      disposition: { kind: "none" },
      note: "Module declarations only (L3-L10).",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/response_completion.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: Anthropic Messages traffic)",
      },
      note: "observe_completion L40 checks content-type text/event-stream L48; SseCompletionTracker L292 parses SSE lines (event: L333, data: L337) and decodes each event's data into the typed inbound ClaudeEventType {type} L302 (L357); finish_event L353 maps error and message_stop into ClaudeResponseCompletion for pin publication.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/response_completion/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/response_completion.rs:404-405 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: Anthropic Messages traffic)",
      },
      note: "Claude Messages request handling. Reads x-claude-code-session-id at L217 (OnSuccess pin publication) and L617 (pin release), copies of affinity_admission.rs:14. claude_provider_unreachable_response L854 writes a hand-built Anthropic error body literal {type:error,error:{type:provider_unreachable,message}} L857 to the Claude Code client; other error bodies come from server::claude_selection_unavailable_response / http_error_response. Upstream response bytes are relayed L255-L266. The Claude Code Anthropic Messages client is not one of the six outside edges (no send-site rows, as for server/response_body.rs). failure_class_from_outcome L795 / failure_class_from_proxy_error L816 are diagnostic labels only.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_r12_tests/attempt_capture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_r12_tests.rs:20-21, a module itself declared #[cfg(test)] at claude_edge/server_pipeline.rs:865-867",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_r12_tests/attempt_telemetry.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_r12_tests.rs:22-23, a module itself declared #[cfg(test)] at claude_edge/server_pipeline.rs:865-867",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_r12_tests/listener_path.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_r12_tests.rs:24-25, a module itself declared #[cfg(test)] at claude_edge/server_pipeline.rs:865-867",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_r12_tests/listener_path/maintenance_reauth.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_r12_tests/listener_path.rs:33-34, itself declared at claude_edge/server_pipeline_r12_tests.rs:24-25 under the #[cfg(test)] module at claude_edge/server_pipeline.rs:865-867",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_r12_tests/selection_responses.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_r12_tests.rs:26-27, a module itself declared #[cfg(test)] at claude_edge/server_pipeline.rs:865-867",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/server_pipeline_tests/acceptance_regressions.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/server_pipeline_tests.rs:17-18, a module itself declared #[cfg(test)] at claude_edge/server_pipeline.rs:861-863",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/upstream_endpoint.rs",
      disposition: { kind: "none" },
      note: "Fixed Anthropic origin https://api.anthropic.com L66 and debug-only isolated override URL validation L40; URL construction only, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/claude_edge/upstream_endpoint/tests.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at claude_edge/upstream_endpoint.rs:85-86 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/claude_route_diagnostics.rs",
      disposition: { kind: "none" },
      note: "Debug-only closed stderr diagnostic lines (write_to L74) with an allow-list of stage and cause labels L27-L70; not a protocol message on any edge.",
    },
    {
      file: "crates/codex-router-proxy/src/credential_runtime.rs",
      disposition: { kind: "none" },
      note: "Credential resolver factory, refresh lease and task supervisor wiring, preloaded affinity secret provider L365; no serde, no message shape.",
    },
    {
      file: "crates/codex-router-proxy/src/db_write_actor.rs",
      disposition: { kind: "none" },
      note: "Bounded SQLite write actor: DbWriteCommand L63 carries derived fields (ProviderErrorClassification, PreviousResponseAffinityOwnerRecord, SessionAccountAffinity, active-client mirror rows) to state-store writes L301-L388; no wire message is parsed or built.",
    },
    {
      file: "crates/codex-router-proxy/src/db_write_actor/session_affinity_scheduler.rs",
      disposition: { kind: "none" },
      note: "Debounce scheduler for SessionAccountAffinity writes keyed by provider and session id L97; in-process channel only, no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/db_write_actor/test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          'cfg(test)-only module: declared at db_write_actor.rs:988-990 (#[cfg(test)] #[path = "db_write_actor/test_support.rs"] mod test_support)',
      },
    },
    {
      file: "crates/codex-router-proxy/src/headers.rs",
      disposition: { kind: "none" },
      note: "Header and HeaderCollection carriers; sanitize_headers_for_upstream L110 strips hop-by-hop, auth, cookie and x-codex-router-token headers (L130-L151) and injects authorization and chatgpt-account-id L119-L124. HTTP headers, no message bodies.",
    },
    {
      file: "crates/codex-router-proxy/src/http_sse.rs",
      disposition: {
        kind: "excluded",
        reason:
          "outside Project 2's protocols (model proxy: OpenAI Responses and Anthropic Messages traffic)",
      },
      note: "passive_claude_quota_observations L186 parses Anthropic anthropic-ratelimit-unified-{5h,7d}-utilization and -reset response headers L205-L218 into WindowObservation; record_buffered_response_owner L851 calls extract_response_id_from_body (http_affinity.rs:197) on buffered Responses bodies. Otherwise local auth, selection and forwarding; bodies pass through.",
    },
    {
      file: "crates/codex-router-proxy/src/http_sse/http_affinity.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "extract_response_id_from_body L197: whole-body JSON, else SSE data: lines (skipping [DONE]) L204-L215, each parsed by extract_json_response_id L221 for a top-level id L228 into PreviousResponseId. Also AffinityOwnerRecordingBody L11 and audit event builders (Router audit log, not a protocol).",
    },
    {
      file: "crates/codex-router-proxy/src/http_sse/http_types.rs",
      disposition: { kind: "none" },
      note: "HttpProxyRequest L10, UpstreamHttpRequest L116, HttpProxyResponse L276, streaming variants and transport traits: HTTP carrier types holding opaque body bytes, no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/http_sse/tests.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at http_sse.rs:71-73 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/lib.rs",
      disposition: { kind: "none" },
      note: "Module list L3-L21; proxy_tests.rs is the #[cfg(test)] mod tests at L29-L31.",
    },
    {
      file: "crates/codex-router-proxy/src/local_auth.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Local token gate (TOKENLESS_GENERATION L10). extract_presented_local_token_from_request L99 rejects auth carriers in the query L137, cookie L150 and the top-level keys of a JSON body L157-L169 (raw-text fallback L171) against the Router's own carrier-name list L274; the body scan parses the Responses request body (serde_json::Value, raw-text fallback) and reads its top-level key names; it is the HTTP twin of websocket/response_metadata.rs:158 (group auth-carrier-key-scan).",
    },
    {
      file: "crates/codex-router-proxy/src/maintenance_actor.rs",
      disposition: { kind: "none" },
      note: "SQLite maintenance actor: MaintenanceHint L68 retention and rollup hints run against the state store L169-L265; the test completion channel L600 is cfg(test). No message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/provider_error.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "Responses error-envelope classifiers into ProviderErrorClassification L17: classify_provider_error_envelope L71 (HTTP: top-level type error or object error L157, error.code and error.type L162), classify_responses_websocket_error_envelope L97 (type error -> error, type response.failed -> response.error L105-L113), token tables L176 and L183, and bounded prefix scanners for bodies over 64 KiB L187 (HTTP) and L237 (WebSocket, type error only). OpenAI Responses model traffic relayed by the N2 byte relay, outside Project 2's protocols: no inventory or send-site rows.",
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/affinity_record_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:177-178, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/affinity_seed_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:181-182, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/auth_rejection_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:225-226, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/credential_record_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:189-190, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/credit_seed_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:185-186, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/loopback_request_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:193-194, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/quota_seed_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:197-198, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/selection_record_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:201-202, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/test_identity_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:205-206, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/proxy_tests/upstream_record_fixtures.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at proxy_tests.rs:209-210, inside proxy_tests.rs, the #[cfg(test)] mod tests of lib.rs:29-31",
      },
    },
    {
      file: "crates/codex-router-proxy/src/routes.rs",
      disposition: {
        kind: "excluded",
        reason:
          "outside Project 2's protocols (model proxy: OpenAI Responses and Anthropic Messages traffic)",
      },
      note: "classify_route L74-L98: method, path and upgrade table (POST /v1/responses, POST /v1/responses with upgrade, POST /anthropic/v1/messages, GET /v1/models, POST /v1/memories/trace_summarize, /v1/responses/compact, /v1/images/generations, /v1/images/edits) into RouteKind L18; is_claude_edge_path L102 (/anthropic prefix); Method L7, RouteClass L62.",
    },
    {
      file: "crates/codex-router-proxy/src/secret_store_factory.rs",
      disposition: { kind: "none" },
      note: "ProxyRuntimeSecretStore type alias L10 and a cfg(test) store opener L13-L30; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server.rs",
      disposition: { kind: "none" },
      note: "Loopback server composition: imports and the server/* module list L161-L197; no message shapes of its own.",
    },
    {
      file: "crates/codex-router-proxy/src/server/auth_reloader.rs",
      disposition: { kind: "none" },
      note: "LocalAuthReloader L8; WebSocket subprotocol auth-carrier word check L39; hyper header copying L44-L63; scan limits L69-L72. No message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/connection_diagnostics.rs",
      disposition: { kind: "none" },
      note: "Loopback connection diagnostic labels rendered to stderr L38-L43; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/db_affinity.rs",
      disposition: { kind: "none" },
      note: "Adapters from the HTTP affinity recorder and provider-error observer traits to the DB write and maintenance actors L34-L183; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/errors_and_adapters.rs",
      disposition: { kind: "none" },
      note: "LoopbackRouterRuntimeError L31 and ServerBindError L70; the raw TCP HTTP adapter, request parser and response writers L109-L421 are all #[cfg(test)] and write only HTTP status lines and headers. No message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/hyper_request.rs",
      disposition: {
        kind: "excluded",
        reason: "outside Project 2's protocols (model proxy: OpenAI Responses traffic)",
      },
      note: "split_precommit_http_response L194 scans the first 64 KiB of an upstream Responses HTTP/SSE body through provider_error_body_from_http_buffer and classify_provider_error_envelope L230-L232 to retry on quota exhaustion; precommit_probe_should_continue_for_success_status L251 reads an SSE event: error prefix; PrecommitAuthRecoveryScope L151 repeats the POST /v1/responses route literal. request_metadata_prefix_is_complete_json L119 checks only JSON validity to allow replay, no field read.",
    },
    {
      file: "crates/codex-router-proxy/src/server/loopback_bind.rs",
      disposition: { kind: "none" },
      note: "Loopback bind address, listener and HTTP/WebSocket upgrade header switchpoint L185; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/protocol_handler.rs",
      disposition: {
        kind: "excluded",
        reason:
          "outside Project 2's protocols (model proxy: OpenAI Responses traffic; Router GET /healthz)",
      },
      note: "Writes HTTP responses to the local Codex on the Responses edge: the relayed upstream response L445 and Router signal bodies via quota_state_unavailable_response L393 L457, http_error_response L432 L438 L478 and all_accounts_exhausted_response L494 (credential failures L431 L468 L492 and empty_response sites carry no body). router_compatibility_response L544-L567 serializes RouterCompatibility (codex-router-core) on GET /healthz for the Host: Router vocabulary between Router and Host, not one of the six outside edges, so no send site.",
    },
    {
      file: "crates/codex-router-proxy/src/server/response_body.rs",
      disposition: {
        kind: "excluded",
        reason:
          "outside Project 2's protocols (model proxy: OpenAI Responses and Anthropic Messages traffic)",
      },
      note: "provider_error_body_from_http_buffer L122 classifies a whole buffer, else SSE data: lines L127-L139; record_affinity_owner_from_async_body L45 runs that and extract_response_id_from_body on relayed bodies. HTTP bodies for the local Codex reuse the WebSocket signal frames: http_error_response L280 (ROUTER_NO_OPENAI_ACCOUNTS_SIGNAL L291 as HTTP 503), all_accounts_exhausted_response L508 (ROUTER_ALL_ACCOUNTS_EXHAUSTED_SIGNAL, in-body status 429, HTTP 503), quota_state_unavailable_response L520. Claude edge: claude_selection_unavailable_response L319 maps UnavailableReason to Anthropic error bodies built by claude_api_error_response L430 (json! L438-L451, literal fallback L453, Retry-After L460); captured_claude_provider_response L467 relays captured bytes; CapturedClaudeProviderErrorResponse L310.",
    },
    {
      file: "crates/codex-router-proxy/src/server/runtime_config.rs",
      disposition: { kind: "none" },
      note: "LoopbackRouterRuntimeConfig L33 and LoopbackRouterRuntime start, serve and maintenance scheduling L224-L783; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/runtime_lifecycle.rs",
      disposition: { kind: "none" },
      note: "Connection join handling, UTC-day maintenance claims and detached-handler supervision L24-L97; no message shapes.",
    },
    {
      file: "crates/codex-router-proxy/src/server/tests.rs",
      disposition: {
        kind: "excluded",
        reason: "cfg(test)-only module: declared at server.rs:199-201 under #[cfg(test)]",
      },
    },
    {
      file: "crates/codex-router-proxy/src/server/tests_part1.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at server/tests.rs:6-7, inside the #[cfg(test)] mod tests of server.rs:199-201",
      },
    },
    {
      file: "crates/codex-router-proxy/src/server/tests_part2.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at server/tests.rs:8-9, inside the #[cfg(test)] mod tests of server.rs:199-201",
      },
    },
    {
      file: "crates/codex-router-proxy/src/server/tests_part3.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test)-only module: declared at server/tests.rs:10-11, inside the #[cfg(test)] mod tests of server.rs:199-201",
      },
    },
  ],
  sendSites: [],
};
