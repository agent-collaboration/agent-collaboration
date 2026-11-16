import type { InventoryRow } from "../schema-audit-types.ts";

const R = "crates/agent-session-tui/src/session_picker/";
const tui = [{ crate: "agent-session-tui" }];
const fixtures = [
  { crate: "agent-session-tui" },
  { crate: "codex-router-quota-reset-test-harness" },
];

export const rows: InventoryRow[] = [
  {
    file: `${R}filter/picker_filters.rs`,
    line: 66,
    owner: "codex",
    kind: "content-parser",
    defines:
      "source_matches: classifies Codex Thread.source / threadSource strings by hand: Interactive = source cli|vscode and threadSource not system|exec|app_server|subagent|guardian_review|memory_consolidation (L66-L77); Subagents = source subagent or threadSource subagent|guardian_review|memory_consolidation (L80-L84); same literal lists at collaboration-service codex_session_inventory.rs:61 and codex-native-integration stored_thread_query.rs:318",
    usedBy: tui,
    duplicateGroup: "session-source-mapping",
    disposition: "duplicate",
    fixedBy: "D1",
  },
  {
    file: `${R}filter/picker_filters.rs`,
    line: 88,
    owner: "codex",
    kind: "json-in-string",
    defines:
      "SessionPickerRecord.source carries the Codex SessionSource object as JSON text (written by agent-session-inventory picker_runtime_inventory.rs:179-183 serde_json::to_string); re-parsed with serde_json::from_str and probed for subagent/subAgent keys (L88-L91)",
    usedBy: tui,
    duplicateGroup: "json-in-string",
    disposition: "malformed",
    fixedBy: "D1",
  },
  {
    file: `${R}details/details_client_inventory/mod.rs`,
    line: 147,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "DetailsClientCapabilities::matches_query: SessionRef endpoint_id flattened with String::from and compared with literal endpoint id claude-local to accept a native Claude target",
    usedBy: tui,
    duplicateGroup: "endpoint-id-literals",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}fork/picker_fork_confirmation.rs`,
    line: 185,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "ForkConfirmation::capture_native: SessionRef endpoint_id flattened with String::from and compared with literal endpoint id claude-local before a native Claude fork",
    usedBy: tui,
    duplicateGroup: "endpoint-id-literals",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}new_session/new_session_client_inventory.rs`,
    line: 91,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "qualify_capabilities: hand builds ProviderBindingIdentity from EndpointInventory ChannelDescription::ExternalProvider fields plus inventory.service_epoch as CodexGeneration (L91-L101) for the one Cursor Create binding; called by agent-collaboration session_client_composition.rs:39",
    usedBy: [{ crate: "agent-session-tui" }, { crate: "agent-collaboration" }],
    duplicateGroup: "provider-binding-identity",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/picker_screen_scope.rs`,
    line: 40,
    owner: "router-vocabulary",
    kind: "json-in-string",
    defines:
      "row_key: opaque local ListRowKey for a picker row, localCodex:{id} or hostedCodex:/hostedProvider:/nativeClaude: followed by the canonical serde_json::to_string(SessionRef) (L40-L53); used only as the picker's own row identity for focus and grouping (picker_surface_view.rs:88, picker_grouping.rs:46,64), never parsed back or sent; an opaque key, not a SessionRef conversion",
    usedBy: tui,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_receipts/mod.rs`,
    line: 19,
    owner: "router-vocabulary",
    kind: "rust-type",
    defines:
      "PickerIdentityReadback (serde tag kind: localCodex {sessionId} / hostedCodex / hostedProvider / nativeClaude {target: SessionRef}) inside PickerOutcomeReadback L63: debug-fixture encoding of SessionPickerIdentity (which has no Serialize) written to picker-outcome.json (L203), read by quota-reset-test-harness provider_filter_pty_test.rs",
    usedBy: fixtures,
    duplicateGroup: null,
    disposition: "boundary-encoding",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_receipts/mod.rs`,
    line: 284,
    owner: "router-vocabulary",
    kind: "session-ref-conversion",
    defines:
      "recording_preparer: EndpointRef built by hand with literal endpoint id codex-local for the fixture PreparedSourceLaunch",
    usedBy: fixtures,
    duplicateGroup: "endpoint-id-literals",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_receipts/mod.rs`,
    line: 291,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "recording_preparer: CodexGeneration built as json! {serviceEpoch,generation} and decoded with serde_json::from_value",
    usedBy: fixtures,
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_scenarios/mod.rs`,
    line: 64,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "fixture SessionRef built as json! {endpoint:{serviceId,endpointId},sessionId} and decoded with serde_json::from_value: claude-local at L64, codex-fixture-ssh at L122",
    usedBy: fixtures,
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_scenarios/picker_fixture_fork_preparation.rs`,
    line: 66,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "FixtureForkPreparation::prepare: CodexGeneration built as json! {serviceEpoch,generation} and decoded with serde_json::from_value",
    usedBy: fixtures,
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/fixtures/picker_fixture_scenarios/picker_fixture_grouping.rs`,
    line: 103,
    owner: "router-vocabulary",
    kind: "history-renderer",
    defines:
      "R19 grouping fixture: push-line headers written by hand (direct message, board notification, wake, question, thread activity at L57-L77) plus a router://…/push/… link (L103), copies of the collaboration-protocol push-line text that session_grouping.rs:150 parses back with parse_push_line",
    usedBy: fixtures,
    duplicateGroup: "push-line-text",
    disposition: "duplicate",
    fixedBy: "D2",
  },
  {
    file: `${R}screen/fixtures/provider_filter_storyboard_fixture.rs`,
    line: 129,
    owner: "router-vocabulary",
    kind: "hand-json",
    defines:
      "source-qualified-outcome storyboard: SessionRef built as json! {endpoint:{serviceId,endpointId},sessionId} and decoded with serde_json::from_value",
    usedBy: fixtures,
    duplicateGroup: "session-ref-conversion",
    disposition: "duplicate",
    fixedBy: "D2",
  },
];
