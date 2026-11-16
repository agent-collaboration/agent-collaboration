import type { LedgerSection } from "../schema-audit-types.ts";

const R = "crates/agent-session-tui/src/";
const P = `${R}session_picker/`;
const portSend =
  "no node tap covers agent-session-tui; the port is injected (production composition in agent-collaboration, agent-session-inventory) and no test records the resulting MCP tools/call at the WireFrame layer for this caller";

export const section: LedgerSection = {
  root: "crates/agent-session-tui",
  files: [
    {
      file: `${R}lib.rs`,
      disposition: { kind: "none" },
      note: "crate root: declares session_picker and the cfg(test) picker_control_test_support module (L4); no message shape",
    },
    {
      file: `${R}picker_control_test_support.rs`,
      disposition: {
        kind: "excluded",
        reason: "cfg(test) module declared at crates/agent-session-tui/src/lib.rs:4",
      },
      note: "re-exports agent-collaboration-tui test-support drivers",
    },
    {
      file: `${P}client/client_action_state/mod.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "builds SessionClientEffect Managed {OperationId::generate L65/L232, SourceManagedAction Create/Load/Resume} and Native {SourceNativeClaudeAction New/Resume/Fork} requests (agent-session-inventory types) and submits them through the injected SessionClientExecutor at L147 (send site); reads the SourceReply at L150",
    },
    {
      file: `${P}client/client_action_view/mod.rs`,
      disposition: { kind: "reads", owners: ["mcp", "router-vocabulary"] },
      note: "renders typed SourceReply results to text: ConversationCreateOutcome Created/CreatedWithoutSettings/Pending (L99-L103), ConversationOperationResult Completed/Pending/Running (L104-L108), SourceProviderActionResult, SourceProviderOperationRead (L113-L116); check_operation L25 queues a ReadManaged effect carrying the retained OperationId (sent via client_action_state.rs:147)",
    },
    {
      file: `${P}client/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}details/conversation_preview_state.rs`,
      disposition: { kind: "none" },
      note: "picker-local cache of SessionConversationPreview keyed by SessionPickerIdentity (agent-session-inventory types); no protocol shape defined, converted or sent",
    },
    {
      file: `${P}details/details_client_inventory/mod.rs`,
      disposition: { kind: "sends", owners: ["mcp", "router-vocabulary"] },
      note: "DetailsClientInspector port called at L80 (send site: client inventory read, endpoints_list on the selected source); matches_query L137 compares SessionRef endpoints and flattens endpoint_id to compare with literal claude-local at L147 (inventory row)",
    },
    {
      file: `${P}details/details_dialog_actions.rs`,
      disposition: { kind: "none" },
      note: "Details action dispatch and OSC52 clipboard payload (session id / cwd strings); no protocol shape",
    },
    {
      file: `${P}details/details_dialog_state.rs`,
      disposition: { kind: "none" },
      note: "terminal UI dialog state: DetailsAction, ClipboardRequestStatus, SessionDetailsDialog; no protocol shape",
    },
    {
      file: `${P}details/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}details/picker_detail_view.rs`,
      disposition: { kind: "reads", owners: ["mcp"] },
      note: "renders typed record fields: NativeClaudeBackgroundState to label (L132-L146), ProviderSessionState to label (L257-L277), session id, job handle; display only, conversation snippets come pre-built in SessionConversationPreview (no Codex/ACP item rendering here)",
    },
    {
      file: `${P}details/session_id_clipboard.rs`,
      disposition: { kind: "none" },
      note: "crossterm OSC52 clipboard writer; terminal output, no protocol shape",
    },
    {
      file: `${P}filter/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}filter/picker_filters.rs`,
      disposition: { kind: "converts", owners: ["codex"] },
      note: "source_matches hand-classifies Codex source/threadSource strings (L66-L84) and re-parses the JSON text in record.source (L88) - two inventory rows; root/provider/runtime filters compare typed fields",
    },
    {
      file: `${P}filter/provider_filter_model.rs`,
      disposition: { kind: "none" },
      note: "terminal UI filter state: SessionProviderFilter matched against SessionProviderFamily/ProviderKind (L26-L33), filter choices; no protocol shape",
    },
    {
      file: `${P}fork/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}fork/picker_fork_confirmation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "capture_native flattens SessionRef endpoint_id and compares with literal claude-local (L185, inventory row); capture compares typed service ids (L123, L143)",
    },
    {
      file: `${P}fork/picker_fork_view.rs`,
      disposition: { kind: "none" },
      note: "fork confirmation dispatch to picker outcome; no protocol shape",
    },
    {
      file: `${P}grouping/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}grouping/picker_grouping.rs`,
      disposition: { kind: "none" },
      note: "list grouping rows and group-by actions; uses row_key from picker_screen_scope.rs:40 (row there); live Local::now read at L32",
    },
    {
      file: `${P}grouping/session_group_preferences/mod.rs`,
      disposition: { kind: "none" },
      note: "private UI preference store agent-sessions-ui.sqlite (screen sessions, key group-by, SessionGroupFacet stored value); private storage, not one of the four protocols",
    },
    {
      file: `${P}grouping/session_grouping/mod.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "started_by_group L148 parses the first user message with collaboration_protocol::parse_push_line and groups by PushKind; the parser is owned by collaboration-protocol",
    },
    {
      file: `${P}mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations and re-exports; cfg(test) test_support declared at L43",
    },
    {
      file: `${P}new_session/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: `${P}new_session/new_session_client_inventory.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "qualify_capabilities reads EndpointInventory and hand builds ProviderBindingIdentity at L91 (inventory row); NewClientInventoryLoader port called at L145 (send site)",
    },
    {
      file: `${P}new_session/new_session_form/mod.rs`,
      disposition: { kind: "none" },
      note: "terminal UI New draft, machine/client/run choices and availability reasons; no protocol shape",
    },
    {
      file: `${P}new_session/picker_new_scope.rs`,
      disposition: { kind: "none" },
      note: "New dialog action declarations; no protocol shape",
    },
    {
      file: `${P}screen/fixtures/fixture_data.rs`,
      disposition: { kind: "none" },
      note: "test/harness fixture (cfg(any(test, feature quota-reset-test-harness)) at screen/fixtures/mod.rs:3): typed SessionPickerRecord values; no hand-built protocol JSON",
    },
    {
      file: `${P}screen/fixtures/mod.rs`,
      disposition: { kind: "none" },
      note: "feature-gated fixture module declarations",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_preferences.rs`,
      disposition: { kind: "none" },
      note: "debug fixture preference directory check (TUI_SESSIONS_FIXTURE_PREFERENCES_DIR); no protocol shape",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_receipts/mod.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "debug harness (feature quota-reset-test-harness + debug_assertions, fixtures/mod.rs:9): PickerIdentityReadback L19 serializes SessionPickerIdentity, recording_preparer hand builds EndpointRef codex-local L284 and CodexGeneration via json!/from_value L291 (three inventory rows); NewSessionReadback/FixtureRunMode are receipt-only",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_scenarios/mod.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "harness scenarios (feature quota-reset-test-harness, fixtures/mod.rs:12): SessionRef via json!/from_value L64, L122 (inventory row); typed ProviderSessionSummary L69; stand-in SessionClientPorts L198",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_scenarios/picker_fixture_fork_preparation.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "harness fork preparer: CodexGeneration via json!/from_value L66 (inventory row)",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_scenarios/picker_fixture_grouping.rs`,
      disposition: { kind: "defines", owners: ["router-vocabulary"] },
      note: "harness R19 metadata: hand-written push-line headers L57-L77 and router:// push link L103 (inventory row); fixed fixture clock",
    },
    {
      file: `${P}screen/fixtures/picker_fixture_scenarios/picker_fixture_source_updates.rs`,
      disposition: { kind: "none" },
      note: "harness record loader gated on a fixture Unix socket handshake update/prepared (L42, L50); test control bytes, not one of the four protocols",
    },
    {
      file: `${P}screen/fixtures/provider_filter_storyboard_fixture.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "harness storyboard: SessionRef via json!/from_value L129 (inventory row); RouterConnectionRegistry config JSON L89, L119 is the private router registry file format, not a protocol; other fixtures typed",
    },
    {
      file: `${P}screen/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations; cfg(test) picker_layout_budget L34, picker_r15_contract_test L59",
    },
    {
      file: `${P}screen/picker_actions.rs`,
      disposition: { kind: "none" },
      note: "SessionsPickerKey and SessionsPickerOutcome (in-process picker result wrapping agent-session-inventory types); no serde, no protocol shape",
    },
    {
      file: `${P}screen/picker_choice.rs`,
      disposition: { kind: "none" },
      note: "terminal UI PickerChoice",
    },
    {
      file: `${P}screen/picker_choice_scope/mod.rs`,
      disposition: { kind: "none" },
      note: "terminal UI combo-box fields and choice dispatch",
    },
    {
      file: `${P}screen/picker_choice_scope/picker_choice_projection/mod.rs`,
      disposition: { kind: "none" },
      note: "terminal UI option keys (client:/machine:/filter: Debug text); no protocol identity",
    },
    {
      file: `${P}screen/picker_component/mod.rs`,
      disposition: { kind: "none" },
      note: "component wiring: passes injected ports to workers (sends happen at source_reload_worker/mod.rs:168, new_session_client_inventory.rs:145, details_client_inventory/mod.rs:80, client_action_state/mod.rs:147); conversation preview via agent-session-inventory from_rollout_source L168",
    },
    {
      file: `${P}screen/picker_dialog_scope.rs`,
      disposition: { kind: "none" },
      note: "terminal UI dialog declarations",
    },
    {
      file: `${P}screen/picker_display_text.rs`,
      disposition: { kind: "none" },
      note: "terminal text fitting",
    },
    {
      file: `${P}screen/picker_entrypoint/mod.rs`,
      disposition: { kind: "none" },
      note: "terminal lifecycle and harness entry; caller supplies record loader and client ports; SESSION_PICKER_OUTCOME stdout marker (L207) is harness output",
    },
    {
      file: `${P}screen/picker_help_content.rs`,
      disposition: { kind: "none" },
      note: "help text",
    },
    {
      file: `${P}screen/picker_layout_budget.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-session-tui/src/session_picker/screen/mod.rs:34",
      },
      note: "row budget arithmetic for tests",
    },
    {
      file: `${P}screen/picker_list_view.rs`,
      disposition: { kind: "none" },
      note: "row geometry constant",
    },
    {
      file: `${P}screen/picker_model/mod.rs`,
      disposition: { kind: "none" },
      note: "picker state model: filters, focus, visible rows over typed SessionPickerRecord; no protocol shape",
    },
    {
      file: `${P}screen/picker_model/picker_runtime_view.rs`,
      disposition: { kind: "none" },
      note: "terminal UI runtime view enum",
    },
    {
      file: `${P}screen/picker_model/picker_visible_window.rs`,
      disposition: { kind: "none" },
      note: "visible-window arithmetic",
    },
    {
      file: `${P}screen/picker_r15_contract_test/picker_r15_expectations.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-session-tui/src/session_picker/screen/picker_r15_contract_test.rs:558 (parent cfg(test) at session_picker/screen/mod.rs:59)",
      },
      note: "R15 test oracles",
    },
    {
      file: `${P}screen/picker_rendering.rs`,
      disposition: { kind: "none" },
      note: "cfg(test) text snapshot renderer plus iocraft_cell_count; no protocol shape",
    },
    {
      file: `${P}screen/picker_request.rs`,
      disposition: { kind: "none" },
      note: "SessionsPickerRequest (in-process picker input); no serde",
    },
    {
      file: `${P}screen/picker_screen_scope.rs`,
      disposition: { kind: "converts", owners: ["router-vocabulary"] },
      note: "row_key L40 flattens SessionPickerIdentity / SessionRef into string keys via serde_json::to_string (inventory row); rest is action declarations",
    },
    {
      file: `${P}screen/picker_surface/mod.rs`,
      disposition: { kind: "none" },
      note: "surface context and dialog lifecycle",
    },
    {
      file: `${P}screen/picker_surface_view.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "renders typed NativeLiveCoverage endpoint service/endpoint ids and conflict session ids as notice text (L240-L252, display only); rows keyed by row_key (L88)",
    },
    {
      file: `${P}source/mod.rs`,
      disposition: { kind: "none" },
      note: "module declarations; cfg(test) native_catalogue_publication_test L3",
    },
    {
      file: `${P}source/native_catalogue_publication_test/native_warning_waits.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-session-tui/src/session_picker/source/native_catalogue_publication_test.rs:593 (parent cfg(test) at session_picker/source/mod.rs:3)",
      },
      note: "test waits on source-page settlement",
    },
    {
      file: `${P}source/picker_machine_controls.rs`,
      disposition: { kind: "none" },
      note: "terminal UI machine browsing state over RouterRegistryRead",
    },
    {
      file: `${P}source/picker_machine_selection.rs`,
      disposition: { kind: "none" },
      note: "machine choice commit into the reload port",
    },
    {
      file: `${P}source/picker_source_view.rs`,
      disposition: { kind: "none" },
      note: "source-view publication and checkpoint restore over typed PickerRecordsSnapshot; no protocol shape",
    },
    {
      file: `${P}source/source_action_state.rs`,
      disposition: { kind: "reads", owners: ["router-vocabulary"] },
      note: "builds typed SourceSessionAction New/Resume/Fork (L87, L121) and validates the PreparedSourceLaunch reply against the captured action, service id, endpoint and client profile (L191-L198)",
    },
    {
      file: `${P}source/source_action_worker.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary", "mcp"] },
      note: "production_preparer L24 calls agent-session-inventory prepare_configured_source (SSH SourceOperation::Prepare; the remote agent-sessions answers through its CollaborationClient MCP calls) - send site; generic latest-read scheduler",
    },
    {
      file: `${P}source/source_reload_port.rs`,
      disposition: { kind: "none" },
      note: "watch-channel reload port",
    },
    {
      file: `${P}source/source_reload_progress.rs`,
      disposition: { kind: "none" },
      note: "terminal UI read-progress states and notices",
    },
    {
      file: `${P}source/source_reload_worker/mod.rs`,
      disposition: { kind: "sends", owners: ["router-vocabulary", "mcp"] },
      note: "SessionsPickerRecordLoader called at L168 (send site: session inventory pages); validate_source_page L256 and source_snapshot_matches L314 check typed SourceInventoryResult rows against SourceInventoryBinding endpoints",
    },
    {
      file: `${P}source/source_reload_worker/qualified_alias_projection/mod.rs`,
      disposition: { kind: "none" },
      note: "alias labels from typed SourceInventoryBinding endpoints; no protocol shape",
    },
    {
      file: `${P}source/source_reload_worker/source_alias_groups.rs`,
      disposition: { kind: "none" },
      note: "alias grouping of typed Qualified results; no protocol shape",
    },
    {
      file: `${P}test_support/mod.rs`,
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-session-tui/src/session_picker/mod.rs:43",
      },
      note: "mock-terminal capture helpers",
    },
  ],
  sendSites: [
    {
      file: `${P}client/client_action_state/mod.rs`,
      line: 147,
      edge: "mcp",
      sends:
        "client action via SessionClientExecutor (agent-collaboration session_client_composition.rs:100-124 to agent-session-inventory source_client_action.rs): Managed Create/Load through ConversationClient, whose ExternalProvider API arm makes the composite conversation_create/conversation_load tools/call (collaboration-client conversation_client.rs:614, conversation_session_operations.rs:122); Resume as provider_session_inspect, conversation resume and operation wait; each preceded by endpoints_list binding checks and, for Load/Resume, provider_sessions_list stored-metadata pages (source_provider_actions.rs:83-206,257-335); ReadManaged (conversation operation show); configured sources run the same over SSH; Native Claude actions start a local process, not MCP",
      owner: { crate: "agent-session-tui" },
      origin: "product",
      coverage: { kind: "uncovered", reason: portSend },
    },
    {
      file: `${P}details/details_client_inventory/mod.rs`,
      line: 80,
      edge: "mcp",
      sends:
        "DetailsClientInspector: client inventory read (endpoints_list on the selected source, then provider binding checks); for a hosted provider target, read_provider_details also sends endpoints_list binding checks, provider_sessions_list stored-metadata pages and provider_session_inspect (agent-collaboration details_client_composition.rs:113-124; agent-session-inventory source_provider_actions.rs:31-54,257-335)",
      owner: { crate: "agent-session-tui" },
      origin: "product",
      coverage: { kind: "uncovered", reason: portSend },
    },
    {
      file: `${P}new_session/new_session_client_inventory.rs`,
      line: 145,
      edge: "mcp",
      sends:
        "NewClientInventoryLoader: SourceOperation::ClientInventory (endpoints_list on the selected source)",
      owner: { crate: "agent-session-tui" },
      origin: "product",
      coverage: { kind: "uncovered", reason: portSend },
    },
    {
      file: `${P}source/source_reload_worker/mod.rs`,
      line: 168,
      edge: "mcp",
      sends:
        "SessionsPickerRecordLoader: per source, endpoints_list discovery, native sessions list pages with a conditional per-session native inspect, and provider sessions list pages for hosted/configured sources (agent-session-inventory picker_runtime_inventory.rs:63,112; provider_picker_inventory.rs:30,88,204)",
      owner: { crate: "agent-session-tui" },
      origin: "product",
      coverage: { kind: "uncovered", reason: portSend },
    },
    {
      file: `${P}source/source_action_worker.rs`,
      line: 24,
      edge: "mcp",
      sends:
        "prepare_configured_source: SSH SourceOperation::Prepare for Resume/Fork/New; the remote agent-sessions makes the Router MCP calls",
      owner: { crate: "agent-session-tui" },
      origin: "product",
      coverage: {
        kind: "uncovered",
        reason:
          "no node tap covers agent-session-tui; the hop leaves this machine over SSH and no test records the remote MCP tools/call at the WireFrame layer",
      },
    },
  ],
};
