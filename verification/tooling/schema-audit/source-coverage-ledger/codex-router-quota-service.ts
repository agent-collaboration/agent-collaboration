import type { LedgerSection } from "../schema-audit-types.ts";

export const section: LedgerSection = {
  root: "crates/codex-router-quota-service",
  files: [
    {
      file: "crates/codex-router-quota-service/src/credential_resolution.rs",
      disposition: { kind: "none" },
      note: "AsyncProviderCredentialResolver trait (:9) over codex_router_auth ResolvedProviderCredential; in-process credential port, no serde, no message of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/credit_usage_summary.rs",
      disposition: { kind: "none" },
      note: "pure decimal formatting of CreditBalance (normalized_parts :4, compact_credit_balance :14, credit_usage_compact_summary :92) for terminal display; no serde, no JSON.",
    },
    {
      file: "crates/codex-router-quota-service/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations only (:2-9).",
    },
    {
      file: "crates/codex-router-quota-service/src/quota.rs",
      disposition: { kind: "none" },
      note: "module glue: imports codex_router_auth::live_quota UsageResponse/WindowPair/usage_url/reset_credits_url (:16-19, ChatGPT backend HTTP quota API, out of scope), declares submodules and re-exports (:100-175); defines only constants and current_unix_seconds (:94). No own serde type.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/claude_quota_fetcher.rs",
      disposition: { kind: "none" },
      note: "Anthropic OAuth usage HTTP API (https://api.anthropic.com/api/oauth/usage, :10), out of scope: reqwest GET with anthropic-beta header (:54-59) and Deserialize ClaudeUsageResponse (:81), ClaudeLegacyUsageWindow (:89), ClaudeUsageLimit (:95) parsed into QuotaRefreshProviderResponse (:101-166). Not Codex app-server, ACP, MCP or Router vocabulary.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_background_refresh_worker.rs",
      disposition: { kind: "none" },
      note: "background refresh thread BackgroundQuotaRefreshWorker (:12) / BackgroundQuotaRefreshRuntime (:18) driving refresh_quota_store_paths_with_dependencies_and_floor_notifier (:148); diagnostics are stdout lines; no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_refresh_history.rs",
      disposition: { kind: "none" },
      note: "maps QuotaRefreshProviderWindow to router-private SQLite PersistedQuotaHistoryObservation (:30-73, :93-116) and purges history (:122); private storage, no serde, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_refresh_provider.rs",
      disposition: { kind: "none" },
      note: "ChatGPT backend HTTP quota API (OpenAI quota API, out of scope): reqwest GET usage_url with ChatGPT-Account-ID (:243-256), serde_json Value then from_value::<UsageResponse> (:269-278), GET reset_credits_url (:292-305) and a key-normalizing Value walk reset_credits_available_from_json (:391-448); projects into in-process QuotaRefreshProviderResponse (:105) / QuotaRefreshProviderWindow (:141). The Value->UsageResponse step decodes one HTTP body, not two Router types. Not Codex app-server account/rateLimits.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_refresh_service.rs",
      disposition: { kind: "none" },
      note: "refresh orchestration over router-private SQLite (AsyncSqliteStateStore :166, PersistedSelectorQuotaWindow :743, PersistedQuotaSnapshot :778, record_responses_refresh_success :831) and the provider HTTP port (fetch_quota :329, :519); WeeklyQuotaFloorIntent (:43) mapped onto WebSocketQuotaFloorNotifier methods (:78-92), an in-process call into the proxy. No serde, no message of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_refresh_service/quota_refresh_helpers.rs",
      disposition: { kind: "none" },
      note: "weekly floor intent from stored selector windows (:16-116), credit refresh attempt bookkeeping (:118-166), stdout diagnostics (:168-186), QuotaServiceError -> QuotaRefreshErrorClass (:188-207, internal storage class); no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_reset_pace_projection.rs",
      disposition: { kind: "none" },
      note: "pure pace/meter projection into QuotaPaceSnapshot and ResetPaceViewModel (:10-76, :241-359) and terminal meter text (:361-465); no serde, no JSON.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_route_selection_projection.rs",
      disposition: { kind: "none" },
      note: "SelectorQuotaInput -> DisplayQuotaWindow (:11), stored history -> QuotaRunRateObservation (:108-120), display windows -> BurnDownAccountInput (:122-159); in-process selection inputs, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_formatting.rs",
      disposition: { kind: "none" },
      note: "local CLI stdout: tab-separated write_quota_plain (:12-77) and write_quota_json serializing JsonQuotaStatusReport (:152-163) for `codex-router quota status --json` (caller codex-router-cli/src/quota/quota_status_command.rs:39). CLI report, not MCP/ACP/Codex; no collaboration-mcp or collaboration-protocol consumer.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_formatting/quota_status_value_formatting.rs",
      disposition: { kind: "none" },
      note: "terminal cell/label formatting from selection enums (quota_human_group :18, format_routing_reason :437, format_next_use_from_routing_reason :478, quota_window_status_from_selector_status :560); display strings only, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_json.rs",
      disposition: { kind: "none" },
      note: "Serialize-only local CLI JSON report JsonQuotaStatusReport (:11), JsonQuotaStatusAccount (:58), JsonCreditUsage (:177), JsonSalvageTieKey (:245), JsonWindowSlots (:251), JsonWindowSlot (:271), JsonRunRateEstimate (:321), JsonQuotaWindow (:350) plus enum-to-string tables (:385-497) for `quota status --json` stdout. Consumed only by codex-router-cli quota_status_command.rs:39; not reachable from any MCP tool, ACP or Codex edge.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_loader.rs",
      disposition: { kind: "none" },
      note: "read-only SQLite load of accounts, selector inputs, refresh statuses and active-client counts (:19-53, :71-342), RuntimeCredentialStoreStatus -> CredentialStoreAvailability (:55-69), credit_usage_status (:344-386); private storage projection, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_metrics.rs",
      disposition: { kind: "none" },
      note: "telemetry_hash (:7) and OpenTelemetry counters/gauges (:18-132, :161-193), feature-gated tracing diagnostic (:135-159); telemetry, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_projection.rs",
      disposition: { kind: "none" },
      note: "QuotaStatusReport -> QuotaStatusViewModel for the quota TUI (:25-103, :195-249, :349-393), CredentialMaintenanceState -> QuotaLoginState (:251-276); terminal UI state, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_status_view_model.rs",
      disposition: { kind: "none" },
      note: "in-process report types QuotaStatusReport (:16), CredentialStoreAvailability (:34), SelectionProjectionSource (:82), CreditUsageStatus (:116), QuotaStatusRow (:170), DisplayQuotaWindow (:289), ActiveClientMirrorStatus (:356); no Serialize/Deserialize.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/quota_unknown_freshness.json",
      disposition: { kind: "none" },
      note: "golden bytes of the local CLI JsonQuotaStatusReport (quota status --json) included by the cfg(test) quota_status_projection_test.rs:228; CLI report fixture, not a message of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota/test_fixtures.rs",
      disposition: { kind: "none" },
      note: 'cfg(any(test, feature = "test-support")) at quota.rs:174 (not test-only, so not excluded): QuotaStatusReport/QuotaStatusRow fixtures (:58-292), capture width assertions (:294-359) and ANSI-to-SVG capture writers (:400-540); no serde, no protocol message.',
    },
    {
      file: "crates/codex-router-quota-service/src/quota_display_facts.rs",
      disposition: { kind: "none" },
      note: "display facts QuotaAccountStatus (:7), QuotaRowStatus (:35), QuotaPoolCensus (:64), QuotaFreshnessCensus (:120), QuotaHeaderFacts (:143); terminal UI state, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_display_model.rs",
      disposition: { kind: "none" },
      note: "TUI view data QuotaStatusViewModel (:6), QuotaStatusAccountViewModel (:19), QuotaLoginState (:48), ResetPaceViewModel (:118), QuotaDisplayWindow (:173); no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_refresh_outcome.rs",
      disposition: { kind: "none" },
      note: "invocation-local AccountRefreshOutcome (:5) / AccountRefreshStatus (:11) merge rules (:44-77); no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset.rs",
      disposition: { kind: "none" },
      note: "quota_reset module declarations/re-exports (:5-17) and QuotaResetError (:21), a thiserror enum for the ChatGPT reset-credit HTTP API; no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/credential_authority.rs",
      disposition: { kind: "none" },
      note: "read-only sqlx account row query (:221-249) and secret-store AccountCredentialBundle read (:291-294) into never-serialized PinnedResetAuthority (:65) with a SHA-256 CredentialFingerprint (:321-349); private storage/secret format, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/provider_protocol.rs",
      disposition: { kind: "none" },
      note: "ChatGPT backend reset-credit HTTP API (OpenAI quota API, out of scope): Deserialize ConsumeResetCreditCode (:30) and ConsumeResetCreditResponse (:38), LiveResetAccountAuth (:13), HttpLiveQuotaResetProvider (:45). Not Codex app-server, ACP or MCP.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/provider_protocol/bounded_http_transport.rs",
      disposition: { kind: "none" },
      note: "ChatGPT backend HTTP quota API (out of scope): Serialize ConsumeResetCreditRequest {redeem_request_id, credit_id} (:222) POSTed to reset_credits_url/consume (:116-135), from_slice ConsumeResetCreditResponse -> KnownConsumeOutcome (:165-184), GET usage_url decoded as UsageResponse for the weekly window (:189-208), GET reset_credits_url (:210-218), bounded body read (:72-108). Not Codex app-server account/rateLimits.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/provider_protocol/provider_response_validation.rs",
      disposition: { kind: "none" },
      note: "ChatGPT backend reset-credit HTTP API (out of scope): Deserialize ResetCreditsResponse (:56) / ResetCreditPayload (:61) -> LiveResetCredit (:17-53), status allowlist (:77-84), RFC 3339 parser (:86-149).",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/quota_reset_session_composition.rs",
      disposition: { kind: "none" },
      note: "composes one reset session from LiveResetAuthorityReader, HttpLiveQuotaResetProvider and QuotaInteractiveSession (:81-100); factory traits (:28-79); no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_commit_service.rs",
      disposition: { kind: "none" },
      note: "in-process reset authority and provider ports (ResetAuthority :172, ResetAuthorityReader :187, ResetServiceProvider :201), CredentialAuthorityError/QuotaResetError -> RenderSafeFailure (:104-119, :161-170), ResetWorkflowService (:312-574); provider traffic is the ChatGPT reset-credit HTTP API, out of scope. No serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_credit_policy.rs",
      disposition: { kind: "none" },
      note: "in-process reset-credit domain: generation newtypes (:4-60), RenderSafeFailure (:67), port results (:115-150), LiveResetCredit (:165), ValidatedCreditInventory (:175), validate_credit_inventory (:334), reset_credit_is_eligible (:368); no serde, not one of the four protocols.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor.rs",
      disposition: { kind: "none" },
      note: "command-session owner QuotaInteractiveSession (:87) over tokio mpsc intents and watch snapshots (:133-166), select loop (:169-222), intent and task-completion handling (:224-473); in-process channels to the quota TUI, no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor/live_reset_inspection.rs",
      disposition: { kind: "none" },
      note: "inspection authority startup and spawned usage/inventory reads (:57-112), confirmation binding (:19-55); in-process orchestration, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor/reset_commit_revalidation.rs",
      disposition: { kind: "none" },
      note: "revalidation authority read, fresh usage/inventory tasks and commit authorization (:19-247); in-process orchestration, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor/reset_effect_execution.rs",
      disposition: { kind: "none" },
      note: "RedeemRequestIdFactory minting the provider redeem_request_id string (:30-43), ResetClock (:45-59), GenerationAllocator (:62), SessionTaskOutput (:85); in-process, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor/reset_presentation_protocol.rs",
      disposition: { kind: "none" },
      note: "in-process presentation port despite the name: ResetSessionIntent (:18), ResetWorkflowSnapshot (:119), ResetCreditDisplayRecord (:87), ResetSessionPorts over tokio mpsc/watch (:334); no serde, never on a wire.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_session_supervisor/reset_session_state.rs",
      disposition: { kind: "none" },
      note: "snapshot publication and attempt identity checks (:21-53); in-process, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_workflow_reducer.rs",
      disposition: { kind: "none" },
      note: "reducer module declarations and re-exports (:3-18).",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_workflow_reducer/correlated_effect_contracts.rs",
      disposition: { kind: "none" },
      note: "in-process reducer correlation OperationCorrelation (:13), CorrelatedRequest (:52), CorrelatedOutcome (:102), WorkflowPhase (:179), OperationActivity (:199); reset-session correlation, not Router operation/receipt vocabulary; no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_workflow_reducer/reset_workflow_state.rs",
      disposition: { kind: "none" },
      note: "reducer state InspectionStart (:17), WorkflowResult (:63), WorkflowIntent (:86), WorkflowActivities (:110), ResetWorkflow (:132); in-process, no serde.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_reset/reset_workflow_reducer/state_transition_reducer.rs",
      disposition: { kind: "none" },
      note: "pure ResetWorkflow::reduce state machine (:96-165) incl. ConsumeUnknownReason -> RenderSafeFailure mapping (:360-377) inside the reset domain; no serde, no outside-edge message.",
    },
    {
      file: "crates/codex-router-quota-service/src/quota_service_error.rs",
      disposition: { kind: "none" },
      note: "QuotaServiceError thiserror enum (:7); no serde, not on any outside edge.",
    },
  ],
  sendSites: [],
};
