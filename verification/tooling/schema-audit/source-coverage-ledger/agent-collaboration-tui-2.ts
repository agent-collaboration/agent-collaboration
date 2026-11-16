import type { LedgerSection } from "../schema-audit-types.ts";

// See agent-collaboration-tui-1.ts: no Router, MCP, ACP or Codex dependency,
// no network client and no send site on any outside edge in this crate.
const testSupportMod = "crates/agent-collaboration-tui/src/test_support/mod.rs";
const frameFixture =
  "rendered terminal frame fixture compared by cfg(test) dialog layout tests; no JSON or message shape";

export const section: LedgerSection = {
  root: "crates/agent-collaboration-tui",
  files: [
    {
      file: "crates/agent-collaboration-tui/src/dialog/help/help_content.rs",
      disposition: { kind: "none" },
      note: "HelpContent/HelpContextLine/HelpInputConvention (help_content.rs:3,8,13) consumer-supplied Help text; no serde",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/help/help_control.rs",
      disposition: { kind: "none" },
      note: "HelpControl (help_control.rs:71) suspends a dialog/dropdown model and opens the Help surface in the in-memory SurfaceStack; terminal UI state",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/help/help_dialog.rs",
      disposition: { kind: "none" },
      note: "HelpDialog/HelpFrame components (help_dialog.rs:53,137) listing source-scope actions; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/help/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the Help dialog only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and re-exports of the dialog chrome only",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/pinned_context.rs",
      disposition: { kind: "none" },
      note: "PinnedContext (pinned_context.rs:5) display text plus bracketed tag and width fitting; no serde",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/.gitattributes",
      disposition: { kind: "none" },
      note: "git attributes marking two .ansi fixtures binary",
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/details-48x12.txt",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/details-wide.txt",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/dialog-no-fixed-slots.ansi",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/dialog_key_hints_wide_120x36.ansi",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/options-120x36-linux.txt",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/dialog/test_fixtures/options-120x36.txt",
      disposition: { kind: "none" },
      note: frameFixture,
    },
    {
      file: "crates/agent-collaboration-tui/src/foundation/dialog_sizing.rs",
      disposition: { kind: "none" },
      note: "DialogPresentation/DialogAxisSize/DialogMargins (dialog_sizing.rs:86,40,139) per-axis cell sizing; terminal layout only",
    },
    {
      file: "crates/agent-collaboration-tui/src/foundation/key_chord.rs",
      disposition: { kind: "none" },
      note: 'KeyChord FromStr/Display (key_chord.rs:7,48,109) parses key-binding text such as "Ctrl+Enter" into iocraft KeyCode; terminal key handling, not a protocol message',
    },
    {
      file: "crates/agent-collaboration-tui/src/foundation/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations and the KeyChord re-export only",
    },
    {
      file: "crates/agent-collaboration-tui/src/foundation/theme.rs",
      disposition: { kind: "none" },
      note: "UiTextRole/UiSurface/TextRole/SurfaceRole/Theme/UiGlyph palette and glyph tables (theme.rs:5,58,98,111,121,174); TUI-local enums mapped to colours, no Router vocabulary",
    },
    {
      file: "crates/agent-collaboration-tui/src/input/dispatch_frame_marker.rs",
      disposition: { kind: "none" },
      note: "test-support feature only (input/mod.rs:1): InputDispatchFrameWriter (dispatch_frame_marker.rs:103) appends an OSC 7770 `dispatch=<n>` terminal marker after each synchronized frame for verification; terminal output, not one of the four protocols",
    },
    {
      file: "crates/agent-collaboration-tui/src/input/input_dispatch_receipt.rs",
      disposition: { kind: "none" },
      note: "test-support feature only (input/mod.rs:3): Serialize InputDispatchReceipt, SurfaceReadback, InputEventReadback, KeyCodeReadback, MouseKindReadback (input_dispatch_receipt.rs:47,60,76,98,123) written as JSONL to a fixture-owned file (:318); readback of terminal input routing, not one of the four protocols and on no outside edge",
    },
    {
      file: "crates/agent-collaboration-tui/src/input/input_owner.rs",
      disposition: { kind: "none" },
      note: "InputOwner (input_owner.rs:38) single terminal-event subscriber routing keys/mouse to focus targets and actions; Routed (:14) is a TUI-local outcome enum",
    },
    {
      file: "crates/agent-collaboration-tui/src/input/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations only; dispatch_frame_marker and input_dispatch_receipt gated on the test-support feature (input/mod.rs:1,3)",
    },
    {
      file: "crates/agent-collaboration-tui/src/lib.rs",
      disposition: { kind: "none" },
      note: "crate root: module declarations and re-exports; test_support compiled under cfg(test) (lib.rs:20) or the test-support feature (lib.rs:22)",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/focus_model.rs",
      disposition: { kind: "none" },
      note: "FocusModel/TargetSpec/TargetId/GroupId (focus_model.rs:187,103,36,55) keyboard focus traversal; TargetId is a TUI target string, not a Router identity",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/hit_regions.rs",
      disposition: { kind: "none" },
      note: "HitRegion component and per-draw target geometry staging (hit_regions.rs:86,52); terminal pointer hit testing",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/hit_regions_model.rs",
      disposition: { kind: "none" },
      note: "HitMapSnapshot/TargetRegion/SurfaceRegion/HitRegions (hit_regions_model.rs:24,12,19,59) derived draw data; no serde",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/mod.rs",
      disposition: { kind: "none" },
      note: "module declarations only",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/overlay_host.rs",
      disposition: { kind: "none" },
      note: "OverlayHost/SurfacePanel/AnchoredPanel components (overlay_host.rs:114,68,244) placing overlay surfaces; terminal rendering only",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/surface_handle.rs",
      disposition: { kind: "none" },
      note: "SurfaceStackHandle/SurfaceContext and use_surface_stack (surface_handle.rs:29,192,168) reactive in-memory stack ownership; terminal UI state",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/surface_stack.rs",
      disposition: { kind: "none" },
      note: "SurfaceStack/Overlay/OverlayId/SurfaceId/OverlayLayout (surface_stack.rs:227,206,32,40,89) in-memory overlay stack; OverlayId is a local counter, not a Router identity",
    },
    {
      file: "crates/agent-collaboration-tui/src/surfaces/target_geometry.rs",
      disposition: { kind: "none" },
      note: "TargetGeometry/TargetGeometryObserver (target_geometry.rs:7,14) draw-time geometry observation; terminal layout only",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/color_environment.rs",
      disposition: { kind: "none" },
      note: "test-support helper require_color_environment (color_environment.rs:1) asserting NO_COLOR is unset",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/control_kind_contract.rs",
      disposition: { kind: "none" },
      note: "test-support control contract runner; assert_outcome_snapshots_eq (control_kind_contract.rs:16) canonicalises fixture outcome bytes via serde_json::Value (:27-30) for comparison; test-local snapshot payloads, not one of the four protocols",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/core_models.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:24 (#[cfg(test)] at :23); target/action/surface model builders, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/dialog_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:8 (#[cfg(test)] at :7); DialogScene component fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/dialog_test_frame.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:26 (#[cfg(test)] at :25); StaticDialogFrame layout fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/dispatch_frame_scene.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module: declared only at crates/agent-collaboration-tui/src/test_support/integration_support.rs:4 (#[path] mod), which is #[cfg(test)] at crates/agent-collaboration-tui/tests/integration/main.rs:4 (declared :6); DispatchScene input fixture, no message shapes",
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/event_driver.rs",
      disposition: { kind: "none" },
      note: "test-support EventDriver/EventJournal (event_driver.rs:63,24) feed iocraft TerminalEvents through a mock terminal; SurfaceSnapshot (:14) holds opaque test bytes",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/fixed_slots_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:3 (#[cfg(test)] at :2); FixedSlotsScene dialog fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/frame_grid.rs",
      disposition: { kind: "none" },
      note: "test-support FrameGrid (frame_grid.rs:22) decodes iocraft Canvas::write_ansi CSI/SGR output into styled cells; terminal output parser, not one of the four protocols",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/frame_inspection.rs",
      disposition: { kind: "none" },
      note: "test-support FrameGrid cell and round-panel crop accessors (frame_inspection.rs:4,8)",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/frame_output.rs",
      disposition: { kind: "none" },
      note: "test-support feature only (test_support/mod.rs:34): DispatchBackend/dispatch_render_loop (frame_output.rs:39,100) record synchronized terminal frame bytes",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/grids.rs",
      disposition: { kind: "none" },
      note: "STANDARD_GRIDS terminal size constants (grids.rs:2)",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/help_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:31 (#[cfg(test)] at :30); HelpScene fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/input_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:10 (#[cfg(test)] at :9); input routing Scene fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/integration_support.rs",
      disposition: {
        kind: "excluded",
        reason:
          "cfg(test) module declared at crates/agent-collaboration-tui/tests/integration/main.rs:6 (#[path] mod test_support, #[cfg(test)] at main.rs:4); not declared by the library (test_support/mod.rs has no such module); #[path] re-includes of test-support helpers only",
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/list_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:12 (#[cfg(test)] at :11); ListScene fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/live_action_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:14 (#[cfg(test)] at :13); LiveActionScene fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/mod.rs",
      disposition: { kind: "none" },
      note: "test-support module declarations (compiled under cfg(test) at lib.rs:20 or the test-support feature at lib.rs:22); no shapes of its own",
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/popup_text_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:19 (#[cfg(test)] at :18); PopupTextScene combo fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/receipt_scene.rs",
      disposition: {
        kind: "excluded",
        reason: `cfg(test) module declared at ${testSupportMod}:16 (#[cfg(all(test, feature = "test-support", debug_assertions))] at :15); ReceiptScene input-receipt fixture, no message shapes`,
      },
    },
    {
      file: "crates/agent-collaboration-tui/src/test_support/recording_backend.rs",
      disposition: { kind: "none" },
      note: "test-support CanvasRecordingBackend/recording_terminal_render_loop (recording_backend.rs:18,56) capture rendered Canvas frames",
    },
  ],
  sendSites: [],
};
