import type { LedgerSection } from "../schema-audit-types.ts";

// The crate depends only on iocraft and unicode-width (serde, serde_json,
// futures-util and tokio behind the non-default `test-support` feature;
// crates/agent-collaboration-tui/Cargo.toml). It has no Router, MCP, ACP or
// Codex dependency, no network client and no send site on any outside edge.
const ansiFrameFixture =
  "rendered terminal frame fixture (plain text with ANSI styling) compared by cfg(test) layout tests; no JSON or message shape";

export const section: LedgerSection = {
  root: "crates/agent-collaboration-tui",
  files: [
    {
      file: "crates/agent-collaboration-tui/src/actions/action_title.rs",
      disposition: { kind: "none" },
      note: "ActionTitle (action_title.rs:5): static or closure-built display title for a TUI action; no serde, no protocol vocabulary",
    },
    {
      file: "crates/agent-collaboration-tui/src/actions/mod.rs",
      disposition: { kind: "none" },
      note: "TUI action registry: ActionId/ScopeId string newtypes (mod.rs:26,39), Availability, Placement, ActionDef, ActionRegistry key routing; in-process presentation types, no serde, not Router identities",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/action_projections.rs",
      disposition: { kind: "none" },
      note: "ActionProjections (action_projections.rs:4) and menu section projection of ActionPresentation; pure layout derivation",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/combo/combo_box.rs",
      disposition: { kind: "none" },
      note: "ComboBox and ComboDropdown iocraft components (combo_box.rs:26,68) and label fitting; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/combo/combo_control.rs",
      disposition: { kind: "none" },
      note: "ComboBoxControl (combo_control.rs:51): opens a dropdown overlay and builds its focus targets in the in-memory SurfaceStack; no serde, no outside edge",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/combo/combo_state.rs",
      disposition: { kind: "none" },
      note: "ComboBoxState (combo_state.rs:26) query/highlight/pick model driven by iocraft TerminalEvent; terminal UI state",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/combo/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the combo controls only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/button.rs",
      disposition: { kind: "none" },
      note: "Button/MenuButton components and button_target (button.rs:50,77,138) projecting ActionPresentation; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/editor_window_fixture.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-collaboration-tui/src/controls/fields/text_field.rs:319 (#[path] mod editor_window_fixture, #[cfg(test)] at text_field.rs:317); WindowScene editor test fixture, no message shapes",
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/form_fields.rs",
      disposition: { kind: "none" },
      note: "FormFields/FormField/SelectableRow layout components (form_fields.rs:42,86,114); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the field controls only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/segmented_switch.rs",
      disposition: { kind: "none" },
      note: "SegmentedSwitch component and segmented_targets width projection (segmented_switch.rs:42,74); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/status_value.rs",
      disposition: { kind: "none" },
      note: "StatusKind (status_value.rs:8) maps to a display word and TextRole colour; a TUI-local enum, not a mapping from a Router vocabulary",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/test_fixtures/.gitattributes",
      disposition: { kind: "none" },
      note: "git attributes marking *.ansi fixtures binary",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/test_fixtures/button_reason_wide_120x36.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/test_fixtures/button_sibling_reason_120.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/test_fixtures/button_sibling_reason_76.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/test_fixtures/form_field_legacy_120.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/text_field.rs",
      disposition: { kind: "none" },
      note: "TextFieldState editor model and TextField/FilterField components (text_field.rs:14,277,305); terminal input editing only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/fields/wrapped_reason.rs",
      disposition: { kind: "none" },
      note: "WrappedReason component (wrapped_reason.rs:10); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/.gitattributes",
      disposition: { kind: "none" },
      note: "git attributes marking *.ansi fixtures binary",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/120x36-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/120x36-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/120x36-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/120x36-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/32x24-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/32x24-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/32x24-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/32x24-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/48x12-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/48x12-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/48x12-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/48x12-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/76x30-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/76x30-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/76x30-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/content-scroll-ungrouped/76x30-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/.gitattributes",
      disposition: { kind: "none" },
      note: "git attributes marking *.ansi fixtures binary",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/120x36-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/120x36-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/120x36-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/120x36-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/32x24-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/32x24-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/32x24-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/32x24-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/48x12-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/48x12-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/48x12-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/48x12-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/76x30-marker0-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/76x30-marker0-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/76x30-marker2-end.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/fixtures/ungrouped/76x30-marker2-top.ansi",
      disposition: { kind: "none" },
      note: ansiFrameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/list_row.rs",
      disposition: { kind: "none" },
      note: "ListRow/ListRecordRow components (list_row.rs:25,42) painting selection and group headings; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/list_selection_control.rs",
      disposition: { kind: "none" },
      note: "ListSelectionControl (list_selection_control.rs:55): keyboard/pointer list selection over in-memory focus targets; no serde, no outside edge",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/list_selection_state.rs",
      disposition: { kind: "none" },
      note: "ListRowKey opaque string newtype and ListRows/ListSelectionState (list_selection_state.rs:10,44,103); caller-supplied row keys, no parsing or conversion of Router identities",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the list controls only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/option_list.rs",
      disposition: { kind: "none" },
      note: "OptionKey/SelectOption (option_list.rs:4,16) choice identity and case-insensitive filter match; TUI-local, no serde",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/scroll_list.rs",
      disposition: { kind: "none" },
      note: "ScrollList/ScrollBar/ScrollReveal components and geometry observers (scroll_list.rs:87,171,253); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/list/scroll_model.rs",
      disposition: { kind: "none" },
      note: "ScrollHandle/ScrollPosition (scroll_model.rs:12,38) and navigation_code key-alias translation (:350); terminal UI state",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/menu.rs",
      disposition: { kind: "none" },
      note: "menu_overlay and MenuDropdown/MenuItemRow (menu.rs:127,208,250) build a menu overlay from the action registry; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the controls only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/toolbar.rs",
      disposition: { kind: "none" },
      note: "Toolbar component, toolbar_actions width fitting and toolbar_target (toolbar.rs:14,28,48); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/controls/typography.rs",
      disposition: { kind: "none" },
      note: "UiHeading/UiText/SurfaceHeader/UiColumnHeading/GroupHeading components (typography.rs:28,52,73,92,114); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/dialog_footer.rs",
      disposition: { kind: "none" },
      note: "DialogFooter component measuring action and hint rows (dialog_footer.rs:30); terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/dialog_frame.rs",
      disposition: { kind: "none" },
      note: "DialogFrame component (dialog_frame.rs:80) sizing header, pinned slots, scroll body and footer; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/dialog_header.rs",
      disposition: { kind: "none" },
      note: "DialogHeader component (dialog_header.rs:57) fitting title, PinnedContext and Help; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/dialog_key_hints.rs",
      disposition: { kind: "none" },
      note: "hint_pairs and DialogKeyHints (dialog_key_hints.rs:19,42) render key hints from ActionPresentation; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/dialog_section.rs",
      disposition: { kind: "none" },
      note: "DialogSections/DialogSection layout components (dialog_section.rs:12,23); terminal rendering only",
    },
  ],
  sendSites: [],
};
