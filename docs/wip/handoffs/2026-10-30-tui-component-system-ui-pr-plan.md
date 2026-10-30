> Carried copy of the canonical plan for helpers on other machines. Canonical identity: the Lead's `tmp/plan-workflows/2026-10-30-tui-component-system-ui-pr-r2.md` in the design worktree. Same text; the Lead updates both together.

# Plan: TUI component system — the UI PR

## Canonical record

```text
plan path: ~/code/projects/codex-router.design-tui-component-system/tmp/plan-workflows/2026-10-30-tui-component-system-ui-pr-r2.md
originating planner: plan-implementation
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements:        docs/specs/2026-10-29-tui-component-system/requirements.md
  Specification:       docs/specs/2026-10-29-tui-component-system/specification.md
  Program Design:      docs/specs/2026-10-29-tui-component-system/program-design.md
  Verification Design: docs/specs/2026-10-29-tui-component-system/verification-design.md
  four-document review: tmp/reviews/2026-10-30-tui-component-system-four-document-review.md
    (independent Sol xhigh reviewer; needs-revision at 9cbda49bf, F1–F6;
     same reviewer verified corrections at ff2629877: ready)
  current applicability anchors: design branch design/tui-component-system at fe8c97c53 (owner changes after review: crate renames, TS/Rust-only verification; review F7 original-image route);
    product base origin/main 881f778c7; iocraft pin agent-collaboration/iocraft 829ebd86
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:ui-component-system
  PR topology: one-pr
```

Planned at: branch `design/tui-component-system`, HEAD `fe8c97c53`. Replaces the earlier plan path `2026-10-30-tui-component-system-ui-pr.md` after the owner's renames and languages rule. Tracking: none. The owner's standing rule is to decide operations without asking.

## Goal and scope

Build `agent-collaboration-tui` and move the session picker and the quota screen onto it, delivering U1–U15 and R1–R16 as the four documents state, and proven by the Verification Design's layers A–F. Owner: "fixing claude code can be first and release, then all the UI improvements second PR". This plan is that second PR.

**In:**
- Rename `codex-router-tui` to `agent-collaboration-tui`.
- The crate core: InputOwner, SurfaceStack, FocusModel, HitRegions, ActionRegistry, OverlayHost, Theme.
- The new core components: menu, combo box, standard dialog. The moved and restyled controls.
- The picker and quota cutovers, including the ≡ menu and toolbar on both screens.
- The verification apparatus each layer needs.
- Version bump and release, per `AGENTS.md`.

**Out (non-goals, from the design):**
- Any iocraft change, fork or pin change.
- Publishing the crate.
- The grid feature (Probe 8).
- Windows.
- H1, the unreproduced keyboard outage.
- Any change to what the picker or quota screen do beyond the UI work.
- The Claude launch behaviour itself, which ships first in the Claude-fix PR.

## External dependencies and stand-ins

| Dependency | Needed by | If missing |
| --- | --- | --- |
| Claude-fix PR (`fix/sessions-ux-claude-attach`) merged to `main` | S6. The picker cutover must carry its New-form Client entries ("Claude Code", "Claude Code · background"), Details "Resume in background", and the new terminal-outcome variant. | S1–S5 start from current `main`. Merge `main` into the UI branch when the Claude-fix PR lands, before S6 starts. No stand-in: S6 waits. |
| Sunclaw build and test capacity | every Cargo step (owner: tests run on Sunclaw) | Helpers run there via fleet messages. Check `df` before builds and keep at least 60 GiB free. `cargo clean --profile dev` per worktree at slice end. |
| Isolated signed debug Router and Host on Sunclaw, Haiku | S9 layer F | No stand-in. F stays open, named in the delivery evidence, not waived. |
| Owner at Ghostty (layer C) | S9 | C is temporary. The H2 gap stays named until the owner runs it; it never blocks merge-readiness of the other rows. |

## Current evidence (re-anchored at 881f778c7)

- `crates/codex-router-tui` (3,209 lines) is used by `agent-collaboration`, `agent-session-picker`, `codex-router-cli` and `codex-router-quota-tui`.
- `codex-router-tui` today subscribes in `dialog_interaction.rs:48` (`use_local_terminal_events`), and `DialogButton` uses that helper. The picker routes keys in `picker_input.rs:148-411`. Client actions run inside the mounted picker (`client_action_state.rs:117-140`). Codex launches after teardown (`session_command_dispatch.rs:271-285`).
- Quota's bottom row is built in `quota_help_rendering.rs:339-360`; only `F4 Details` is a clickable `DialogAction`. Its reset quit behaviour is `quota_reset_keyboard_interaction.rs:21-47`, protected by `quota_account_options_test.rs:161-208`.
- Gates in CI:
  - `cargo fmt --all -- --check`;
  - `cargo clippy --workspace --all-targets -- -D warnings`;
  - `cargo nextest run -p workspace-architecture-check --run-ignored only` (module-cycle ratchet);
  - `cargo deny check`;
  - `cargo audit`;
  - `node verification/tooling/check-rust-file-size.ts`;
  - nextest `--profile ci --workspace --features codex-router-cli/keychain-test-support` with the documented excludes.
- Proof apparatus: `scripts/verification/tui/` (`capture-run.ts`, `component-capture-run.ts`, scene contracts) and the Rust PTY driver in `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test*`. Recipes are `.agents/skills/verify-codex-router/references/{tui-journeys,client-actions,claude-background}.md`.
- Probes and demo: `origin/design/tui-component-probes` (P1–P8, demo `9c0180c8`). Probe 1 already serializes with `Canvas::write_ansi`.
- No terminal-screen parser exists in the workspace (no `vt100`, `vte` or similar in `Cargo.lock`).

Proof-layer names: project silent. The plan uses the Verification Design's layers A–F, plus ordinary unit, integration and end-to-end.

## Mechanism choices made here (recorded, reversible)

- **Layer A's frame reader.** This is a test-only parser in `agent-collaboration-tui`'s test support. It reads `Canvas::write_ansi` output: SGR, printable text with `unicode-width` 0.2 widths, and CRLF, ignoring the trailing CRLF. It **rejects any other escape sequence**, so an unknown sequence fails loudly instead of passing. It is chosen over a new dev-dependency because `write_ansi`'s output is a small closed grammar, and a new crate would need `cargo deny`/`cargo audit` admission for a few dozen lines of parsing.
- **Verification languages.** TypeScript and Rust only (owner). The B oracle is wterm's (libghostty) cell grid, not screenshot OCR; no xterm.js (owner). Swift, VHS tapes and shell runners leave the TUI journeys in S0.
- **Layer E's sanctioned call site.** It is `#[expect(clippy::disallowed_methods, reason = "InputOwner is the only terminal subscriber (R5)")]`. This is the one marked boundary; there is no other lint suppression.
- **Old components are removed in S8**, after both consumers have moved. They are not removed in S4, because the consumers still use them until then. Nothing ships with two input paths, because all of this is one PR.
- **Module-cycle ratchet across the rename (S1).** If the baseline lists modules by crate name, the edge set before and after must be identical apart from the name. Prove it with a sorted diff of edges under the name mapping. Any other change stops S1.

## Slices

Each slice: what it covers, where it writes, the Verification Design rows it earns (with command and timing), the wrong behaviour its own proof must fail on, the existing tests it touches, and when to stop.

### S0 — Verifier: TS/Rust grid oracle, journeys and "before" evidence (proof-only)

- **Covers:** the Verification Design's "Journeys still needed" (B), its languages rule, and the baseline half of "Build order".
- **First, the grid oracle (owner: "only use ts or rust").**
  - Qualify wterm as the headless emulator (owner: no xterm.js): `@wterm/core` `TerminalCore` with the `@wterm/ghostty` (libghostty) core, at exact pinned versions. Fixtures with known grids cover named and 24-bit colours, wide glyphs (📂, ✉️, 界, including at overlay-left−1), the kitty query and reply, and resize. Record the choice and why.
  - Feed the Rust PTY driver's bytes into it.
  - Port the existing scene checks in `scene-contracts.ts` and the Swift `verify_screen_*.swift` checks to grid assertions in TypeScript.
  - Add the original-image route in TypeScript: the exact PTY bytes rendered by `@wterm/dom` on the same libghostty core in headless Chrome over the DevTools protocol (no Playwright), saved as original PNGs bound to source, build, viewport and checkpoint. This replaces VHS and ttyd. Show one checkpoint matching its old VHS original's content at the same viewport.
  - Remove the 13 Swift files, the VHS tapes and the `run_capture.sh` / `run_provider_filter_capture.sh` runners from `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/`.
  - Show that every ported journey still fails on its existing wrong-scene and blank controls.
- **Writes:**
  - `scripts/verification/tui/` scene contracts and tapes for: overlay isolation, focus traversal, main-screen keys, the New session dialog, and wide glyph under overlay;
  - the Rust PTY driver's legacy-keys and kitty-protocol modes (answering `CSI ? u`; kitty-encoded keys) and single-write bursts;
  - wide-glyph fixture rows (📂, ✉️, 界 at overlay-left−1);
  - the `verify-codex-router` recipe updates, through `practices-verification-maintenance`.
- **Earns:** the baseline failures. Every new B journey runs against current `main` (881f778c7) in both key modes at 120×36. It must fail where today's defects are real:
  - typed text behind an open menu changes the box behind;
  - Esc quits;
  - Help lists Esc wrongly;
  - Account details has no gap above "Quota windows";
  - the wide-glyph corner is lost.
  Command: `capture-run.ts <plan> --check`, then the run, on Sunclaw. Runs first, in parallel with S1–S4.
- **Pre-change signal** (clarified 2026-10-30 after the Verifier's admission check), three kinds of baseline:
  - **Known defects** must fail today, each for exactly its named defect: Esc quitting the main screen; Help listing Esc wrongly; New session ignoring ↓ and starting focus on Start; Account details with no gap above Quota windows; the wide-glyph corner lost under an existing dialog opened over 📂 rows.
  - **Isolation on overlays that exist today** (Help, New, Details, the filter choosers, quota Account and Pool details): bursts of text, paste, arrows and clicks aimed beneath. Record red or pass per overlay. A pass is a valid baseline, and the journey then guards against regressions.
  - **Entries that are new in this PR** (the ≡ menu, toolbar, dropdowns): record "absent at base", neither red nor pass. Their sensitivity comes from S5's seeded wrong builds; their first green is at S6/S7.
  - An entry timeout is never counted as a red.
- **Existing tests:** the compact Sessions and Quota tapes are kept unchanged here. They are re-baselined only in S9, for the three owner-decided changes.
- **Stop:** a needed journey can't reach its entry through the fixture harness. Return a `verification-design-gap` with the row.

### S1 — Renames and quota-role move (prefactoring)

- **Covers:** Program Design migration phase 1; constraint C1.
- **Writes:**
  - `crates/codex-router-tui` becomes `crates/agent-collaboration-tui` (package and lib name);
  - `crates/agent-session-picker` becomes `crates/agent-session-tui`, and `crates/codex-router-quota-tui` becomes `crates/agent-router-quota-tui` (owner, 2026-10-30);
  - every `Cargo.toml` and `use` path that names them;
  - `WeeklyWindow`/`ShortWindow` move into `codex-router-quota-tui`;
  - workspace `Cargo.toml` and `Cargo.lock`.
- **Consumer:** S2 and all later slices.
- **Proof (characterization):** behaviour unchanged.
  - All existing tests of the four consumers and the crate pass unchanged, apart from path renames: `cargo nextest run --profile ci -p agent-collaboration-tui -p agent-session-picker -p codex-router-quota-tui -p agent-collaboration -p codex-router-cli`.
  - `cargo tree -p agent-collaboration-tui` shows no codex-router dependency.
  - `rg 'codex-router-tui|agent-session-picker|codex-router-quota-tui'` finds no remaining reference outside history and changelogs.
  - The ratchet rule is as above.
- **Pre-change signal:** n/a (rename). The wrong behaviour would be a dropped re-export breaking a consumer, which the compile and test run catches.
- **Existing tests:** all kept. `typography_tests.rs` is repaired: its quota-role cases move with the roles.
- **Quality:** fmt, clippy (workspace), file-size, ratchet, deny.
- **Stop:** the ratchet needs more than the name mapping.

### S2 — Crate core state and layer-A harness (contract)

- **Covers:**
  - E2–E8 bindings: SurfaceStack and Overlay; FocusModel and TargetSpec; ActionRegistry with `ActionDef`, `KeyChord`, `AvailabilityFn`, `Placement { Toolbar, Menu, Hidden }` and duplicate-key rejection;
  - Theme roles from the current shipped `codex-router-tui` roles (base, raised, group panel, text, muted, accent, focused, primary, disabled, dimmed, good/warning/error), each background with a dimmed form;
  - the glyph set;
  - the layer-A test support:
    - burst and frame-separated event driver over `mock_terminal_render_loop`, with event-processed acknowledgements and the initial frame drained;
    - serialized-frame reader;
    - per-surface state snapshot and handler log.
- **Writes:** `agent-collaboration-tui/src/{surface_stack,focus_model,actions,theme}.rs` and `tests/support/`.
- **Consumer:** S3.
- **Proof (unit):**
  - Duplicate key in one scope is rejected. Wrong build: last binding silently wins.
  - Push/pop/restore by `TargetId`, falling back to the first enabled target. Wrong build: restore by index.
  - Focus traversal and groups: a table over target layouts. This is a property, since one rule covers all layouts.
  - Glyph widths: every crate glyph is one cell under `unicode-width` 0.1.14 **and** 0.2.x. Wrong build: ☰. This earns the glyph clause of U12's row.
  - Harness self-tests: the frame reader rejects an unknown escape; the burst driver delivers N events before any redraw. Oracle: the Specification text and Probe 3's recorded event ordering.
- **Borrowed with attribution:** CometixTUI's key-binding parser, scroll helper and focus routines (Program Design "Reuse").
- **Stop:** a binding needs an iocraft API absent at the pin.

### S3 — InputOwner, HitRegions, OverlayHost (vertical at crate level; first integration gate)

- **Covers:**
  - R1, R2, R3 and R11 at component level, plus R10.
  - The two R1 exceptions, routed.
  - Press-only effects; Repeat only for navigation and editing.
  - Hit regions recorded in `pre_component_draw`, keeping only cells that `drawer.canvas().cell()` returns, and published when the root finishes drawing.
  - Initial size taken from the root's props; Resize routed through InputOwner.
  - Overlays as root siblings with the one-cell gutter.
  - Dimmed covered surfaces.
- **Writes:** `agent-collaboration-tui/src/{input_owner,hit_regions,overlay_host}.rs`.
- **Earns, at layer A** (`cargo nextest run --profile ci -p agent-collaboration-tui`, every change, Sunclaw and CI):
  - U1 · R1, A part: every boundary case as a burst and frame-separated; beneath snapshots byte-equal; only R1's exceptions.
  - U2 · R2, A part: beneath inert; dimmed roles outside the overlay-plus-gutter footprint; restored in the closing redraw.
  - U3 · R3, A part: Tab, arrows and click reach; exactly one focus; published map equals draw geometry each frame. Clip cases: fully hidden target; half-visible target; plain `overflow: Hidden` consumer view; nested and bordered; each axis; after resize.
  - U9 · R10, A part: serialized frame keeps border, corners and gutter background with 📂, ✉️ and 界 at overlay-left−1.
  - U10 · R11, A part: Esc pops one overlay; outside click closes a menu, leaves a dialog open, logs nothing beneath; focus restored, or the first enabled target if gone.
- **Pre-change signals (seeded wrong builds; each test must fail):**
  - a render-time focus flag gate (Probe 3);
  - a map from `use_component_rect`;
  - a crate-only clip list;
  - no gutter (Probe 1);
  - an outside click falling through;
  - restore by index.
- **Stop:** state can't be applied synchronously between two events of one burst. That is the design's own reopen condition.

### S4 — Controls and projections (vertical at crate level)

- **Covers:** R5–R9, R12–R15 at component level. The controls are:
  - TextField (crate-owned) and FilterField;
  - Button;
  - MenuButton, which shows its key, and MenuDropdown, with checked items;
  - Toolbar, whose buttons drop from the right when narrow;
  - ComboBox, with R14's rules;
  - SegmentedSwitch and ScrollList;
  - DialogFrame, moved: subscriptions removed, marked primary action, content height for forms only;
  - DialogSection, with its rhythm and tinted group panel;
  - StatusValue;
  - DialogKeyHints;
  - FormFields;
  - HelpDialog, generated, with body swap inside a dialog.
  Also the ActionRegistry projections to menu, toolbar, Help and dialog hints.
- **Writes:** `agent-collaboration-tui/src/controls/*`. Each file stays under 600 lines.
- **Earns, at layer A** (same command):
  - U5 · R9, A part: every dialog key; Ctrl+Enter with a dropdown open closes it unchanged and runs Start once; text-field keys.
  - U6 · R6: names and keys equal across projections; availability flips everywhere in one redraw; a double press in a burst where the first makes the action unavailable runs it once.
  - U7 · R7, A part: Help entries equal the bound pairs; scrolls; body swap restores query, focus and draft byte-for-byte.
  - U8 · R8: switch and checkmarks equal the model after click, menu and key.
  - U11 · R12, A part: text grid shows every element; one blank row between sections and one before actions; one primary; status word present; form height equals content.
  - U13 · R14: still-matching typing keeps the highlight; hidden by own typing moves to the first match; outside removal leaves no highlight and Enter picks nothing; unavailable options are highlightable but not pickable; Esc leaves the field unchanged; "no match" shown.
- **Pre-change signals (seeded wrong builds):**
  - native `TextInput` (Probe 6);
  - availability cached per render;
  - a hand-typed menu label;
  - `SearchSelect`'s first-match reset (Probe 7);
  - highlight by position;
  - doubled or missing section gap.
- **Existing tests:**
  - `dialog_tests.rs`, `dialog_async_tests.rs`, `form_field` cases: **repair** (layout and measurement kept; input now routed by InputOwner).
  - `dialog_action_keyboard_tests.rs`: **remove** in S8. Replaced by S4's DialogFrame/Button key tests, which fail on the same wrong behaviour (a key not activating the focused action).
  - `search_select_tests.rs`: **remove** in S8. Dead contract: the Program Design retires `SearchSelect` for `ComboBox`. ComboBox tests cover the replaced cases.
  - `typography_tests.rs`: kept.
- **Stop:** a control needs its own terminal subscription.

### S5 — Crate acceptance observation (batch boundary)

The paired Verifier independently runs the S2–S4 layer-A suite at the declared head on Sunclaw. It challenges each seeded wrong build: the test must fail on it and pass on the right build. It records "seeded wrong-build sensitivity" as its own evidence, separate from S0's baseline. S6 and S7 start only after this report.

### S6 — Picker cutover (vertical; consumer integration gate)

- **Covers:** migration phase 3; U4, U5, U12 and the picker halves of U1–U3 and U9–U15; R16 for Sessions. Specifically:
  - delete `picker_input.rs`'s key chain, the per-dialog key maps, `picker_machine_input.rs` routing and the hand-written Help;
  - declare scopes, targets and actions;
  - toolbar: ≡ Menu F10 (in Tab order), [+ New], [/ Filter…], [This folder | All];
  - menu: New session…, New background session… (for clients with both modes), Details, Open, Fork, Refresh, the sort and runtime checked items, Help, Quit;
  - New dialog: ComboBoxes for Client and Machine, a Run choice when the client supports both, the "Ctrl+Enter start · Esc close" hint;
  - keys: Enter shows Details; Ctrl+Enter opens; Ctrl+Shift+Enter forks; the Esc ladder; Ctrl+C via `.ignore_ctrl_c()` with the picker outcome read back;
  - initial size from the launch boundary;
  - a consumer `clippy.toml` with the three disallowed methods.
- **Writes:** `crates/agent-session-picker/src/session_picker/*`, plus `crates/agent-collaboration` where the picker root is launched.
- **Earns:**
  - U4 · R4, reused part: `picker_details_navigation_tests.rs`, migrated: Enter inspects and cannot launch; Ctrl+Enter resumes without Details; Ctrl+Shift+Enter shows the fork confirmation. Package nextest, every change.
  - U15 · R5, picker half: workspace clippy with the picker `clippy.toml`.
  - B rows are earned in S9.
- **Pre-change signal:** S0's baseline journeys fail on today's picker. The slice must turn the picker ones green at 120×36 in both key modes before S9.
- **Existing picker tests:**
  - **Keep** (model, identity, inventory, data; no input path): `picker_model_tests`, `picker_source_identity_tests`, `qualified_alias_projection_tests`, `default_inventory_publication_tests`, `native_catalogue_publication_tests`, `ssh_source_publication_tests`, `source_reload_*_tests`, `picker_progressive_read_tests`, `picker_background_tests`, `picker_all_status_tests`, `details_client_membership_tests`, `source_action_tests`, `picker_performance_tests`.
  - **Repair** (drive through InputOwner, same assertions): `picker_details_navigation_tests`, `client_action_component_tests`, `details_client_action_tests`, `source_action_component_tests`, `picker_component_tests`, `picker_interaction_tests`, `picker_pointer_tests`, `picker_fork_tests`, `picker_machine_control_tests`, `provider_filter_component_tests`, `provider_filter_focus_reveal_tests`, `provider_filter_tests`, `new_session_form_tests`, `session_choice_ux_tests`, `session_context_ux_tests`, `form_alignment_tests`, `picker_layout_tests`, `picker_compact_header_tests`, `picker_single_view_tests`, `picker_pane_stability_tests`, `picker_restoration_tests`, `picker_capture_tests`.
    - An assertion that encodes a rule the owner changed is repaired to the new expectation, citing the owner's words: inline choice to dropdown; Esc quitting to Esc never quitting.
    - Any other failing assertion is a regression, not a repair.
  - **Repair** `picker_help_tests`: it now asserts the generated Help's action and key list equals the scope's bindings.
- **Stop:**
  - the Claude-fix PR is not yet merged;
  - a preserved R16 capability has no crate control that can carry it;
  - a repair would change an assertion the owner did not change.

### S7 — Quota cutover (vertical; may run alongside S6 after S5)

- **Covers:** migration phase 4; R4, R13, R15 and R16 for quota. Specifically:
  - toolbar ≡ Menu F10, [Details], [Filters: … ▾], [Pool], [Options], [Floor] when available;
  - menu: every action with its key, then Help and Quit;
  - the bottom row is removed (`render_quota_browse_footer`);
  - Esc never quits; Q quits;
  - Ctrl+C through `.ignore_ctrl_c()` with the quota quit outcome read back;
  - the Committing guard unchanged;
  - dialogs on DialogFrame and DialogSection (Account details, Pool details, options, floor, reset) with the rhythm, group panels and StatusValue;
  - the four 2026-10-23 groups and layout kept;
  - a consumer `clippy.toml`.
- **Writes:** `crates/codex-router-quota-tui/src/*`.
- **Earns:**
  - U4 · R4, quota part: `quota_account_options_test.rs:161-208` kept and passing. Committing ignores Ctrl+C; other phases keep `ExitPrecommit`.
  - U15 · R5, quota half: workspace clippy.
  - U14 · R15, A part: control-kind tests parameterised over picker and quota instances, after S6 lands.
- **Existing quota tests:**
  - **Keep:** the data, presentation, pace, credit and identity tests: `quota_browse_presentation_test`, `quota_browse_rendering_test`, `quota_credit_presentation_test`, `quota_group_observation_test`, `quota_hard_fixture_consistency_test`, `quota_presentation_support_test`, `quota_reset_scenarios_test`, `quota_storyboard_fixture_test`, `quota_storyboard_observation_test`, `quota_ux_identity_test`, `quota_ux_pace_test`, `quota_ux_inventory_caption_test`, `quota_empty_state_test`, `quota_performance_test`, `quota_visual_contract_test`, `quota_browse_compatibility_test`, `quota_compact_layout_test`, `quota_normal_browse_pose_test`, `quota_reset_result_pose_test`, `quota_account_options_paging_test`, `quota_account_options_scroll_test`, `quota_account_options_test`.
  - **Repair** (the input path or the removed bottom row): `quota_choice_dialog_test`, `quota_filter_help_pose_test`, `quota_floor_editor_presentation_test`, `quota_form_alignment_test`, `quota_help_presentation_test` (generated Help), `quota_provider_filters_test`, `quota_reset_presentation_test`, `quota_ux_browse_scroll_test`, `quota_ux_choice_test`, `quota_ux_clarity_test`, `quota_ux_form_test`, `quota_ux_frame_test`, `quota_ux_help_test`, `quota_ux_options_entry_test`, `quota_ux_reset_test`. The same owner-change rule as S6 applies.
- **Stop:** any quota domain guard has to move to make the cutover work.

### S8 — Remove the old input path; structural gate (cleanup + proof)

- **Covers:** "no second input path"; U15 · R5 in full.
- **Writes:**
  - delete `DialogHost`, `DialogInputScope`, `UseDialogInteraction`, `dialog_interaction.rs`'s subscriptions, `SearchSelect` and `ShortcutHelp`;
  - remove `dialog_action_keyboard_tests.rs` and `search_select_tests.rs`, with the S4 replacement proofs named above;
  - add the crate's own `clippy.toml` and the single `expect` in InputOwner;
  - add three negative-control fixtures under the workspace CI command.
- **Earns:** U15 · R5. `cargo clippy --workspace --all-targets -- -D warnings` passes. Each control fails it:
  - a consumer calling `use_terminal_events`;
  - a consumer calling `use_local_terminal_events`;
  - `DialogButton` keeping its listener.
  A render-only control passes. The controls are run as a scripted check, not left in the tree.
- **Stop:** any consumer still needs a removed piece.

### S9 — Acceptance observations (batch boundary; Verifier and owner)

All at the S8 head on Sunclaw, signed debug builds.

- **B, every new journey from S0, both screens, six grids plus same-child resize, legacy and kitty modes:**
  - U1 · R1, B part.
  - U2 · R2, B part.
  - U3 · R3, B part: clicks at all grids and after resize.
  - U4 · R4: main-screen keys, Ctrl+C quit outcome read back on both screens, and the separate Committing case.
  - U5 · R9, B part: New dialog, Ctrl+Enter with a dropdown open, the legacy Tab+Enter fallback, Start reaching the preparation stand-in once.
  - U7 · R7, B part: updated Help checkpoints.
  - U9 · R10, B part.
  - U10 · R11, B part.
  - U11 · R12, B part: Details, Account details, New session, Pool details.
  - U12 · R13: both top rows and menus, narrow drop-off, F10, Tab to the menu, no bottom row.
  - U13 · R14, B part.
- **B, existing journeys, R16:** compact Sessions and Quota tapes, Normal43 and hard-case Quota, plus the two PTY tests. Re-baseline only the dropdown, Esc-on-quota, dialog-rhythm and bottom-row-to-toolbar checkpoints, listing each one.
- **F, R16 public entry:** CLIENT-REAL-ENTRY and the `client-actions.md` and `claude-background.md` cases, including the Claude foreground and background modes, on an isolated signed debug Router and Host, using Haiku. A case not run stays open with its route.
- **D:** before/after originals in a presentation-html gallery for the owner (U11, U14 visual). Captures with real session titles are never committed.
- **C:** owner Ghostty check for physical Ctrl+Enter, Ctrl+Shift+Enter, Ctrl+C, click and wheel (U4, U5, H2). Temporary.
- **Stop:** any row fails. Fix in its owning slice, then rerun only that row's observation plus the package gates.

### Delivery (after S9)

- Bump the workspace version to the next unused version and update `Cargo.lock`, per `AGENTS.md`.
- Get an independent implementation review (`implementation-review`), with a fresh reviewer pinned to the head.
- Wrap up the PR (`implementation-pr-wrapup`): CI green at head and on `main`, threads resolved, a public-safety scan of the diff.
- Release under owner authority: tag, release workflow, Homebrew tap. Never restart the production Router.

## Dependency edges

```text
requires S1 -> S2 -> S3 -> S4 -> S5
requires S5 -> S6, S5 -> S7
requires Claude-fix PR merged -> S6
requires S6 -> S7's U14 parameterised tests
requires S6, S7 -> S8 -> S9
requires S0 -> S9
parallel S0 || S1..S4   (Verifier lane vs Implementer lane)
parallel S6 || S7       (advisory; separate worktree lanes on the one PR branch, merged in)
```

## Obligation-to-proof map

| Row | Earned in | Command or journey |
| --- | --- | --- |
| U1 · R1 | S3 (A), S9 (B) | crate nextest; overlay-isolation journey |
| U2 · R2 | S3 (A), S9 (B) | crate nextest; menu/dropdown/dialog-open scenes |
| U3 · R3 | S3 (A), S9 (B) | crate nextest; focus-traversal journey |
| U4 · R4, C3 | S6, S7 (reused tests), S9 (B, C) | picker and quota nextest; main-screen-keys journey; owner check |
| U5 · R9 | S4 (A), S9 (B, C) | crate nextest; New session dialog journey; owner check |
| U6 · R6 | S4 (A) | crate nextest |
| U7 · R7 | S4 (A), S9 (B) | crate nextest; Help checkpoints |
| U8 · R8 | S4 (A) | crate nextest |
| U9 · R10 | S3 (A), S9 (B) | crate nextest; wide-glyph journey |
| U10 · R11 | S3 (A), S9 (B) | crate nextest; outside-click scenes |
| U11 · R12 | S4 (A), S9 (B, D) | crate nextest; dialog scenes; gallery |
| U12 · R13 | S2 (glyph test), S9 (B) | crate nextest; top-row and menu scenes |
| U13 · R14 | S4 (A), S9 (B) | crate nextest; dropdown scene |
| U14 · R15 | S7 (A, parameterised), S9 (D) | crate nextest; gallery |
| U15 · R5 | S6, S7, S8 (E) | workspace clippy plus three negative controls |
| 10-17 baseline · R16 | S9 (B, F) | existing journeys re-run; public-entry journeys |

Every slice also runs the package gates for what it touched: fmt, `cargo clippy --workspace --all-targets -- -D warnings`, file-size, the module-cycle ratchet and `cargo deny check`. The full workspace nextest runs once, at S8's head before S9, on Sunclaw. Per the owner's rule, any declared red must come from a full package run at that head.

## Risks and false-green watch

- **A observes only serialized frames.** Backend full, diff and resize writes are B's job. A row that is green in A and not yet run in B is not done.
- **One PTY write is not one render batch.** A is the decisive same-batch test; B is transport.
- **The test-only frame reader could mask a sequence it ignores.** It rejects unknown escapes, which is tested in S2.
- **Re-baselining existing journeys could hide a regression.** Re-baseline only the four owner-decided checkpoints, each listed with the owner's words.
- **Large PR.** It is kept to one PR by owner instruction, as several batches (S5, S9) with independent observations. If review shows it can't be read in one pass, the split is reported, not done unasked.
- **Disk on Sunclaw.** Lanes are package-scoped. The full workspace runs only once.

## Stop and replan conditions

- Any slice's stop condition above.
- A design break: R1's synchronous routing fails at the pin, a control must subscribe itself, or a preserved capability can't be carried. Stop, and return to `program-design` with evidence.
- A Verification Design row can't be earned as written. Return `verification-design-gap` with the row.
- Disk below 60 GiB free on Sunclaw. Pause builds and clean the slice's own target first.
