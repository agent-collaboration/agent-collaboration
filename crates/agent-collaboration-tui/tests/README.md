# TUI tests (layer A)

Unit tests live beside their owning source modules as `*_test.rs`.
`integration/` covers public-API, renderer-parity and subprocess contracts. Its
`main.rs` registers the single `integration` Cargo target; `component_contract.rs`
declares the scenario modules. Run only this kind from the workspace root:

```sh
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui --test integration
```

There is no e2e Cargo target in this crate. End-to-end PTY observations belong to
the repository's verification drivers. New e2e tests, if introduced here, belong
in `e2e/` with `main.rs` and one explicit `e2e` Cargo target. Keep loose Rust test
files out of the `tests/` root.

Run from the workspace root:

```sh
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui
```

Use the repository-pinned Nextest tool. Color-sensitive setup fails immediately
when `NO_COLOR` exists, even when empty. Never skip those tests or substitute
uncolored expectations. The pinned renderer owns a private memoized color switch
([iocraft color.rs:242–250](https://github.com/agent-collaboration/iocraft/blob/829ebd86bd07fe2100c096ec6d785875ec2e0c79/packages/iocraft/src/color.rs#L242));
crossterm's public color override cannot affect it. Product code continues to
honor `NO_COLOR`. The harness changes no process environment. A negative run with
`NO_COLOR=1` must fail at setup with the actionable precondition message.

`../src/test_support/frame_grid.rs` reads the actual `Canvas::write_ansi` bytes at their known
dimensions. Its grammar is printable text, SGR, CRLF and CSI K (erase to row end).
Every other escape, including OSC 8, is rejected. It measures text using current
Unicode widths. The glyph contract separately checks the renderer's 0.1.14 table.
This layer observes complete serialized frames, not backend diff/resize writes.
Those remain the independent PTY observation's responsibility.

`../src/test_support/recording_backend.rs` mirrors the pin's mock backend: the same event
channel supplies input, and each output is a cloned `Canvas`. It uses the public
`ui.render_loop().backend(recording_backend).ignore_ctrl_c()` entry because the
pin's `mock_terminal_render_loop` has no Ctrl+C opt-in. The parity self-test uses
the original mock as its reference and compares serialized frames in both input
modes without Ctrl+C. Removing the public opt-in must fail the Ctrl+C rows.
This adapter is shared by unit and integration tests under `cfg(test)`.

Component scenarios live beside their owning modules as `<module>_test.rs` and
share the helpers in `../src/test_support/`. `integration/component_contract.rs` indexes the
cross-module backend-parity and subprocess-frame integration tests; each file in
`integration/` states why it needs that boundary. Representative UI matrices use
`../src/test_support/grids.rs`: 120×36, 76×30, 48×12 and 32×24. Tiny-view and exact
threshold tests retain the sizes needed to exercise their boundary. Native
dialog baselines live in `../src/dialog/test_fixtures/`.

Legacy library characterization tests retain the original mock while
their input path is retired in S8; they exercise no new Ctrl+C row and contribute
no claim about that row. Pure model tests need no terminal driver.

`../src/test_support/event_driver.rs` drains the initial frame, queues an entire burst before
polling the render loop, and records an acknowledgement for every processed event.
The acknowledgement includes the draw count, per-surface bytes and handler log.
An ignored event is proved by that acknowledgement without awaiting a redraw.
Frame-separated mode waits for an explicitly expected changed frame between
events. A processed-acknowledgement assertion can run before a frame wait, so a
missing action fails on its state rather than timing out waiting for its redraw.
Test timeouts are failure guards, never evidence that input was processed.

## R15 consumer control contracts

`../src/test_support/control_kind_contract.rs` is source-reused beside a
consumer's existing `event_driver`, `frame_grid`, `grids` and `recording_backend` modules.
It imports those siblings; it adds no shipping feature or dependency. Include it
with `#[path = "<relative path to that source>"] pub mod control_kind_contract;`
inside the consumer's test-support module.
Source-reuse `../src/test_support/grids.rs` as its `grids` sibling too, so the
four viewport values keep their existing single owner.

Call `assert_control_kind_matrix(screens, build).await`. Each
`ScreenControlContracts { screen, kinds }` declares each of the eight kinds once:
`ControlKindDeclaration::OnScreen { kind, contracts }` or
`ControlKindDeclaration::NotOnScreen { kind, reason }`. An absent kind needs a
nonempty reason grounded in that screen's actual composition. It does not mount
a substitute widget. An undeclared kind or duplicate declaration fails.

`build` receives
`ControlInstanceRequest { screen, kind, viewport }`, the real `EventJournal`,
and `ObservedControlStack`. It returns the mounted real screen/control element.
Connect the screen's existing stack observer to `observed.observer()` and its
existing input outcome observer to `journal.processed(SurfaceSnapshot(...))`.
Use `journal.log_handler(...)` at the existing recording effect seam. Do not
build a lookalike control or add another terminal listener.

Every `OnScreen` declaration retains the existing `ControlKindContract` cases:
screen, kind, viewport, initial paint assertions and nonempty input steps. The
case's screen/kind must match its declaration. Each present kind requires cases
at all four standard grids; multiple cases per grid may cover different states.
The eight declarations cover button, text field, filter field, combo box,
segmented switch, scrolling list, form field and dialog key hints. A combo-box
query built from an internal editor belongs to the combo-box row; it does not
establish a public TextField or FilterField instance.

The runner returns `ControlMatrixSummary { passed_contracts, not_on_screen }`
and reports absence reasons separately. `passed_contracts` increases only after
a real case completes. `NotOnScreenControl { screen, kind, reason }` entries
never count as passes. Carry them as separate applicability rows in the consumer
declaration. The single-case `assert_control_kind_contract` entry is unchanged.

Each `ControlStep` has `ControlInput::Event(TerminalEvent)` or
`ControlInput::Pointer { target, kind }`, `redraw`, exact `expected_snapshot`,
cumulative `expected_handlers` and paint assertions. Pointer positions come
from the current published hit map on the active surface. A disabled/ignored
event still needs an acknowledgement; set `redraw: false` when it changes no
serialized frame. Wrong outcomes are asserted before waiting for paint.

`ControlPaint::{Contains, Absent, Styled, StyledInTarget}` observes the real serialized frame.
`Styled { text, style: FrameStyle }` checks every cell of every occurrence;
palette indices are serialized `Color::AnsiValue`, not named-color aliases.
`StyledInTarget { target, text, style }` confines that assertion to the target's
currently published hit cells on the active surface. Every occupied text cell
must be inside the target; a label elsewhere cannot satisfy it. An absent,
hidden or unpainted target, or missing text inside it, fails. Duplicate labels
outside that target are ignored. Whole-frame `Styled` remains strict.
Supply literal contract oracles, including normal, focused, hovered, disabled
with reason, and selected/checked where applicable. For passive form/key-hint
presentation, exercise the containing real control/action; the line itself must
not become an invented pointer target. Bind the actual list policy and actual
combo option keys. Consumer callback bytes and effect counts remain their own
typed model's projection.

The crate's qualification uses two Button composition profiles across all four
grids and rejects wrong callback bytes, handler observations, paint and styles.
It also rejects undeclared/duplicate kinds, empty absence reasons, misbound real
cases and incomplete grids before mounting. Qualification checks that absent
kinds never mount and remain separate from runtime passes. This qualifies the
runner; each consumer's real-kind cases and explicit applicability declarations
earn its R15 row.
Full/diff/resize transport and physical-terminal observations remain separate.

The pure key parser, focus traversal and measured scroll calculation are adapted
from [CometixTUI ec7c0615](https://github.com/Haleclipse/CometixTUI/tree/ec7c06155482815a4b4a99be5ad4c62675a632a1),
`use_keybinding.rs:1–153`, `focus.rs:121–435`, and
`components/scroll_view.rs:440–459`. They use its MIT option; the complete notice is
in `../COMETIX-LICENSE-MIT`. Its subscriptions, render registration, sibling Tab
sharing and Esc-clears-focus defaults are not adopted.
