# Component contract tests (layer A)

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

`support/frame_grid.rs` reads the actual `Canvas::write_ansi` bytes at their known
dimensions. Its grammar is printable text, SGR, CRLF and CSI K (erase to row end).
Every other escape, including OSC 8, is rejected. It measures text using current
Unicode widths. The glyph contract separately checks the renderer's 0.1.14 table.
This layer observes complete serialized frames, not backend diff/resize writes.
Those remain the independent PTY observation's responsibility.

`support/recording_backend.rs` mirrors the pin's mock backend: the same event
channel supplies input, and each output is a cloned `Canvas`. It uses the public
`ui.render_loop().backend(recording_backend).ignore_ctrl_c()` entry because the
pin's `mock_terminal_render_loop` has no Ctrl+C opt-in. The parity self-test uses
the original mock as its reference and compares serialized frames in both input
modes without Ctrl+C. Removing the public opt-in must fail the Ctrl+C rows.
This adapter exists only in the integration test crate under `cfg(test)`.

The component A scenarios share `component_contract.rs` and this recording
driver. Legacy library characterization tests retain the original mock while
their input path is retired in S8; they exercise no new Ctrl+C row and contribute
no claim about that row. Pure model tests need no terminal driver.

`support/event_driver.rs` drains the initial frame, queues an entire burst before
polling the render loop, and records an acknowledgement for every processed event.
The acknowledgement includes the draw count, per-surface bytes and handler log.
An ignored event is proved by that acknowledgement without awaiting a redraw.
Frame-separated mode waits for an explicitly expected changed frame between
events. A processed-acknowledgement assertion can run before a frame wait, so a
missing action fails on its state rather than timing out waiting for its redraw.
Test timeouts are failure guards, never evidence that input was processed.

The pure key parser, focus traversal and measured scroll calculation are adapted
from [CometixTUI ec7c0615](https://github.com/Haleclipse/CometixTUI/tree/ec7c06155482815a4b4a99be5ad4c62675a632a1),
`use_keybinding.rs:1–153`, `focus.rs:121–435`, and
`components/scroll_view.rs:440–459`. They use its MIT option; the complete notice is
in `../COMETIX-LICENSE-MIT`. Its subscriptions, render registration, sibling Tab
sharing and Esc-clears-focus defaults are not adopted.
