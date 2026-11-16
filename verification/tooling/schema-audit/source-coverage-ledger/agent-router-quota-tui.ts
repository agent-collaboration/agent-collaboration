import type { LedgerSection } from "../schema-audit-types.ts";

/**
 * R51 second pass (lane J): every file opened. The crate has no serde, reqwest, rmcp or tungstenite
 * production dependency (Cargo.toml [dependencies]: chrono, codex-router-core, crossterm, iocraft,
 * agent-collaboration-tui, tokio, tracing, unicode-width, codex-router-quota-service). Its effects
 * leave through caller-supplied closures (QuotaAccountCommands, QuotaRefresher, CreditUsagePolicySaver,
 * CreditUsageRefresher, WeeklyQuotaFloorSaver, QuotaStatusViewModelLoader) and the in-process
 * ResetIntentSender channel of codex-router-quota-service; it writes no frame on any outside edge.
 */
export const section: LedgerSection = {
  root: "crates/agent-router-quota-tui",
  files: [
    {
      file: "crates/agent-router-quota-tui/src/credit_balance_presentation.rs",
      disposition: { kind: "none" },
      note: "terminal UI formatting only: detailed_credit_balance (credit_balance_presentation.rs:6) formats codex_router_core CreditBalance digits; no message shape of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/lib.rs",
      disposition: { kind: "none" },
      note: "module declarations and pub use re-exports (lib.rs:3-111); no type or message shape defined here",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_action_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI action declarations: account_dialog_controls / submit_account_command (quota_account_action_controls.rs:17,270) push QuotaAccountCommand onto the in-process QuotaAccountCommandQueue; no outside-edge send",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_commands.rs",
      disposition: { kind: "none" },
      note: "in-process TUI effect boundary: QuotaAccountLabel, AccountActionTarget, QuotaAccountAction, QuotaAccountCommand, QuotaAccountRefusal, QuotaAccountCommandResult, QuotaOutcome (quota_account_commands.rs:7,33,55,63,69,85,97) are plain Rust types with no serde, passed to a caller closure; account identity (AccountId) is not Router session vocabulary",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_details_view.rs",
      disposition: { kind: "none" },
      note: "terminal UI rendering; login_command_preview (quota_account_details_view.rs:43) formats a codex-router CLI argv string for display, not a Codex/ACP/MCP message",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_groups.rs",
      disposition: { kind: "none" },
      note: "terminal UI layout of QuotaStatusAccountViewModel rows (group_widths, render_account_groups at quota_account_groups.rs:12,150); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_options.rs",
      disposition: { kind: "none" },
      note: "terminal UI state: CreditUsagePolicySaveError, CreditUsageRefreshError, AccountOptionsState, AccountOptionsCommand (quota_account_options.rs:33,54,178,322) are serde-free in-process types sent over a tokio mpsc AccountOptionsCommandPort (quota_account_options.rs:354); no outside-edge shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_options_commands.rs",
      disposition: { kind: "none" },
      note: "in-process command runner: run_account_options_commands (quota_account_options_commands.rs:42) calls caller closures and sends ResetSessionIntent on the quota-service in-process channel (quota_account_options_commands.rs:85,151,499); not one of the six outside edges",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_options_events.rs",
      disposition: { kind: "none" },
      note: "terminal UI key handling: handle_account_options_key_event (quota_account_options_events.rs:32) sends ResetSessionIntent on the in-process quota-service channel (quota_account_options_events.rs:84,138,224); no outside-edge shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_options_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI rendering of credit usage and reset phase text (render_account_options_panel, credit_entitlement_label at quota_account_options_rendering.rs:29,409); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_account_runner.rs",
      disposition: { kind: "none" },
      note: "in-process queue: QuotaAccountCompletion, QuotaAccountCommandQueue, quota_account_command_runner (quota_account_runner.rs:11,38,72) over tokio mpsc/watch; no serde, no outside-edge send",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_background_updates.rs",
      disposition: { kind: "none" },
      note: "iocraft hook wiring: use_quota_background_updates (quota_background_updates.rs:40) runs in-process command loops and a reload interval; no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_body_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI layout: render_quota_body (quota_body_rendering.rs:17); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_browse_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI rendering of account rows and detail lines (render_account_list, render_account_row, render_selected_details at quota_browse_rendering.rs:25,109,203); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_combo_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI combo-box option keys (provider_options/credit_options/open_choice at quota_combo_controls.rs:30,61,113) map TUI option strings to Provider/CreditUsagePolicy; UI state, not a protocol mapping",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_control_browse_view.rs",
      disposition: { kind: "none" },
      note: "iocraft component QuotaControlBrowse (quota_control_browse_view.rs:28); terminal UI layout only",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_control_context.rs",
      disposition: { kind: "none" },
      note: "terminal UI state: QuotaDialogKind, QuotaDialogSession, QuotaRefreshState, QuotaControlContext (quota_control_context.rs:39,48,56,64); quit sends ResetSessionIntent::Shutdown on the in-process channel (quota_control_context.rs:200-203); no outside-edge shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_control_dialog_view.rs",
      disposition: { kind: "none" },
      note: "iocraft component QuotaControlDialog and dialog content builders (quota_control_dialog_view.rs:70,139); terminal UI only",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_control_effects.rs",
      disposition: { kind: "none" },
      note: "iocraft effect hooks: use_quota_control_effects (quota_control_effects.rs:30) observes in-process account receipts and calls the caller QuotaRefresher closure; refresh_result_message (quota_control_effects.rs:179) formats UI text; no outside-edge send",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_control_help.rs",
      disposition: { kind: "none" },
      note: "terminal UI help text: quota_header_help_context, quota_help_content (quota_control_help.rs:5,29); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_detail_panel.rs",
      disposition: { kind: "none" },
      note: "terminal UI panel selection: render_detail_panel (quota_detail_panel.rs:29); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_dialog_context.rs",
      disposition: { kind: "none" },
      note: "terminal UI pinned identity: QuotaDialogIdentity, current_dialog_identity (quota_dialog_context.rs:9,32) project account label/tag; no message shape (inline #[cfg(test)] mod tests at quota_dialog_context.rs:69)",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_dialog_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI overlay open/close: open_quota_dialog, quota_dialog_model, close_dialog (quota_dialog_controls.rs:65,127,264); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_dialog_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI height budget: quota_dialog_panel_height (quota_dialog_rendering.rs:4); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_domain_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI action declarations for options/floor/reset (domain_dialog_controls, floor_controls at quota_domain_controls.rs:29,360); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_fact_presentation.rs",
      disposition: { kind: "none" },
      note: "terminal UI formatting: reset_pace_role, colorize_reset_pace_ansi, reset_pace_summary (quota_fact_presentation.rs:8,20,106) colour and format quota display text; no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_filter_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI filter dialog actions: filter_dialog_controls (quota_filter_controls.rs:18); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_fixture_controller.rs",
      disposition: { kind: "none" },
      note: 'debug/test-support fixture controller (declared under cfg(any(test, all(debug_assertions, feature = "test-support"))) at lib.rs:108-109): QuotaCapturedContext, QuotaFixtureReadback, QuotaFixtureControlAck (quota_fixture_controller.rs:20,42,70) are serde-free in-process readbacks; no message shape',
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_fixture_view.rs",
      disposition: { kind: "none" },
      note: 'debug/test-support fixture mount (declared under cfg(any(test, all(debug_assertions, feature = "test-support"))) at lib.rs:110-111): controlled_quota_fixture, run (quota_fixture_view.rs:35,81) render the real component; no message shape',
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_floor_editor.rs",
      disposition: { kind: "none" },
      note: "terminal UI editor: WeeklyFloorEditorCommand, WeeklyQuotaFloorSaveError, WeeklyFloorEditorState (quota_floor_editor.rs:21,56,89) are serde-free; run_weekly_floor_editor_commands (quota_floor_editor.rs:191) calls the caller saver closure; no outside-edge send",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_frame_observation.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-router-quota-tui/src/lib.rs:97-98 (#[cfg(test)] mod quota_frame_observation)",
      },
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_group_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI grouping actions: grouping_actions (quota_group_controls.rs:15); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_group_preferences.rs",
      disposition: { kind: "none" },
      note: "UI preference writer: QuotaUiPreferenceStore trait, QuotaGroupPreferences, QuotaGroupPreferenceWriter (quota_group_preferences.rs:11,20,24) persist a QuotaGroupFacet through a caller store; private UI preference, not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_grouped_rows.rs",
      disposition: { kind: "none" },
      note: "terminal UI list grouping: grouped_account_rows, account_row_key (quota_grouped_rows.rs:11,81) turn AccountId into a TUI ListRowKey; account identity is not Router session vocabulary (not SessionRef/thread/input/correlation/operation id)",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_grouping.rs",
      disposition: { kind: "none" },
      note: "UI preference enum QuotaGroupFacet with stored_value/from_stored_value (quota_grouping.rs:3,27,35): private preference string, not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_help_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI chrome and footer text: render_quota_route, render_quota_screen, quota_legacy_footer (quota_help_rendering.rs:6,29,57); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_interactive_component.rs",
      disposition: { kind: "none" },
      note: "iocraft interactive root: render_interactive_quota_status (quota_interactive_component.rs:23) builds QuotaControlContext and mounts InputOwner; no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_key_bindings.rs",
      disposition: { kind: "none" },
      note: "terminal key chord list quota_help_keys (quota_key_bindings.rs:2); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_list_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI list selection: initial_account_selection, account_list_control, synchronize_account_selection (quota_list_controls.rs:24,45,79); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_provider_filter_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI toolbar height: quota_filter_controls_height (quota_provider_filter_rendering.rs:10); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_provider_filters.rs",
      disposition: { kind: "none" },
      note: "terminal UI filter state: AccountProviderFilter, DisabledAccountVisibility, QuotaAccountFilters, QuotaFilterState (quota_provider_filters.rs:26,43,60,90); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_refresh_action.rs",
      disposition: { kind: "none" },
      note: "caller-supplied refresh boundary: QuotaRefresher, QuotaRefreshFailure, QuotaRefreshResult (quota_refresh_action.rs:5,9,15) are serde-free in-process types; no outside-edge shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_reset_component_input.rs",
      disposition: { kind: "none" },
      note: "terminal UI reset key handling: handle_quota_reset_key, try_send_reset_intent (quota_reset_component_input.rs:45,194) send ResetSessionIntent on the in-process quota-service channel; not an outside edge",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_reset_detail_content.rs",
      disposition: { kind: "none" },
      note: "terminal UI reset text: ResetDetailRow, activity_label, disabled_reason, credit_status_label (quota_reset_detail_content.rs:10,54,108,130) format quota-service reset states for display; OpenAI reset-credit data, not one of the four protocols (inline #[cfg(test)] mod tests at quota_reset_detail_content.rs:176)",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_reset_detail_rendering.rs",
      disposition: { kind: "none" },
      note: "terminal UI reset document: reset_detail_document, result_document, reset_footer (quota_reset_detail_rendering.rs:301,589,662) render ResetWorkflowSnapshot/WorkflowResult text; not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_reset_keyboard_interaction.rs",
      disposition: { kind: "none" },
      note: "terminal key mapping: ResetKeyAction, reset_key_action (quota_reset_keyboard_interaction.rs:8,21); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_reset_presentation_model.rs",
      disposition: { kind: "none" },
      note: "terminal UI reset target and paging: ResetPaneTarget, credit_page_start, reset_inventory_page_size (quota_reset_presentation_model.rs:7,20,37); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_screen_controls.rs",
      disposition: { kind: "none" },
      note: "terminal UI browse action scope: quota_screen_model, browse_help_content (quota_screen_controls.rs:42,18); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_static_component.rs",
      disposition: { kind: "none" },
      note: "printed terminal layout: render_static_quota_status (quota_static_component.rs:16); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_status_component.rs",
      disposition: { kind: "none" },
      note: "iocraft component switch QuotaStatusComponent (quota_status_component.rs:7); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_status_entrypoint.rs",
      disposition: { kind: "none" },
      note: "terminal entry points write_quota_status_view, run_quota_status_view and QuotaStatusViewOptions (quota_status_entrypoint.rs:20,61,48) render to the terminal; no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_status_props.rs",
      disposition: { kind: "none" },
      note: "iocraft props QuotaStatusComponentProps (quota_status_props.rs:16); terminal UI only",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_status_view_model.rs",
      disposition: { kind: "none" },
      note: "loader closure type alias QuotaStatusViewModelLoader (quota_status_view_model.rs:5); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_test_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-router-quota-tui/src/lib.rs:65-66 (#[cfg(test)] pub mod quota_test_support)",
      },
      note: "its serde_json receipt parsing and json! key readback (quota_test_support.rs:212,246-266) are terminal input receipts, not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_toolbar_layout.rs",
      disposition: { kind: "none" },
      note: "terminal UI toolbar sizing: QuotaToolbarLayout, toolbar_layout (quota_toolbar_layout.rs:10,15); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_typography.rs",
      disposition: { kind: "none" },
      note: "terminal UI colour roles: QuotaTextRole, QuotaText (quota_typography.rs:5,44); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_view_configuration.rs",
      disposition: { kind: "none" },
      note: "terminal UI constants and QuotaInteractionMode (quota_view_configuration.rs:4-14); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_view_model_loader.rs",
      disposition: { kind: "none" },
      note: "production module (lib.rs:105, unconditional; the cfg(test) at lib.rs:100 covers only the use at lib.rs:101): quota_status_view_model_loader (quota_view_model_loader.rs:11) reads Router-local quota storage through codex-router-quota-service load_quota_status_report_with_availability_async; private storage, not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/quota_viewport_trace.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-router-quota-tui/src/lib.rs:94-95 (#[cfg(test)] mod quota_viewport_trace)",
      },
      note: "its serde ViewportSample (quota_viewport_trace.rs:16) is a test viewport trace file, not one of the four protocols",
    },
    {
      file: "crates/agent-router-quota-tui/src/responsive_quota_layout.rs",
      disposition: { kind: "none" },
      note: "terminal UI layout arithmetic: quota_title_row, quota_body_layout, use_quota_status_spinner (responsive_quota_layout.rs:10,136,307); no message shape",
    },
    {
      file: "crates/agent-router-quota-tui/src/test_support.rs",
      disposition: { kind: "none" },
      note: 'test-support module (cfg(any(test, feature = "test-support")) at lib.rs:52-53, not cfg(test)-only): QuotaFixtureControl, QuotaFixtureOptions, quota_control_fixture (test_support.rs:8,20,35) mount the real component; no message shape',
    },
  ],
  sendSites: [],
};
