# Ghostty input harness — authored for S9

This replaces layer C with harness evidence **as far as that evidence reaches**.
The new route is opt-in; the existing default capture route is unchanged.
The S9 product observations remain pending a declared S6/S7 target. A byte
qualification or protocol subprocess test is not a product or physical-key pass.

## Encoder and actual modes

Pinned `@wterm/ghostty` 0.5.7 exports GhosttyCore and no public key encoder.
Its published `dist/index.d.ts`, full GhosttyCore declarations and WASM export
interface were read. The available input-mode getters are `kittyKeyboardFlags`,
`cursorKeysApp`, `mouseTracking` and `mouseEncoding`. The existing PTY driver's
additive `inputModes()` samples those getters after received product output has
fed the same core. It also records current columns/rows. No protocol label or
qualifier-controlled flag is substituted for that state during a product run.

The route therefore uses the [kitty keyboard specification](https://sw.kovidgoyal.net/kitty/keyboard-protocol/)
plus a bounded Ghostty-compatible key subset in
`scripts/verification/tui/ghostty-input-encoding.ts`. This does **not** execute
libghostty's own encoder. Only press events in `ghosttyKeySchema` are supported;
arbitrary modifiers, key releases, layouts, locks and IME are outside this subset.
Printable text uses code points and respects report-all/associated-text flags;
it is not a paste or an IME qualification.

The wterm release is [f2016031](https://github.com/vercel-labs/wterm/tree/f2016031d6381d90ed90bfbe1862a9ee1127acb0).
Its [Ghostty dependency manifest](https://github.com/vercel-labs/wterm/blob/f2016031d6381d90ed90bfbe1862a9ee1127acb0/packages/%40wterm/ghostty/zig/build.zig.zon#L8)
pins Ghostty v1.3.1. Independent literals were checked against these source/tests:

| Deciding case | Ghostty v1.3.1 primary anchor |
| --- | --- |
| Enter/Tab remain CR/TAB with disambiguation alone | [key_encode.zig:1238–1263](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L1238) |
| Shift+Enter and Shift+Tab CSI-u; report-all Enter | [key_encode.zig:1336–1376](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L1336) |
| Unmodified up arrow is CSI A with disambiguation | [key_encode.zig:1627–1650](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L1627) |
| Ctrl+C legacy ETX | [key_encode.zig:2010–2019](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L2010) |
| Function keys, application cursor keys, Home/End, paging | [function_keys.zig:78–99](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/function_keys.zig#L78) |
| Ghostty legacy modified Enter uses CSI 27;modifier;13~ | [function_keys.zig:195–223](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/function_keys.zig#L195) |
| Kitty special-key press event formatting when event reporting is set | [key_encode.zig:1056–1078](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/input/key_encode.zig#L1056) |

The tests compare literal bytes with the new encoder; no Zig test run is claimed.
In particular, Ghostty legacy can distinguish modified Enter using its extended
legacy encoding. That does not establish support in every legacy terminal, nor
permit dropping the generic Tab/Enter and click fallbacks.

## Example Ghostty keybinds

`example-ghostty-keybinds.ts` models this supplied, consumed binding set before
terminal encoding:

| Key | Delivery to program |
| --- | --- |
| PageUp / PageDown | Consumed by scroll_page_up / scroll_page_down; no bytes |
| Home / End | Consumed by scroll_to_top / scroll_to_bottom; no bytes |
| Shift+Enter | Replacement LF (hex 0a), bypassing kitty encoding |
| Other supported keys | Terminal encoder under current PTY modes |

The application [binding-consumption path](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/Surface.zig#L2987)
is separate from terminal encoding. The fixture models the supplied plain
consumed bindings, not performable/unconsumed overrides, inherited configuration,
sequences or discovery of a running Ghostty app.

**Finding:** paging and first/last keys in this fixture cannot reach the picker.
They must not earn a passing picker-navigation observation. Keep the binding
finding for the Lead; do not remove the model or change the picker oracle to
manufacture a pass. Wheel/arrows remain separately observable paths.

## Run and retain

From the repository root:

```sh
node scripts/verification/tui/ghostty-input-qualification.ts FRESH_OUTPUT.json
node scripts/verification/tui/s9-prestage.ts --check
pnpm --dir scripts/verification/tui run typecheck
pnpm --dir scripts/verification/tui run lint
pnpm --dir scripts/verification/tui run format:check
pnpm --dir scripts/verification/tui test
```

Qualification uses the pinned VT core with explicitly controlled flag0/1/31
sequences, both profiles and all six sizes. It labels its mode source controlled,
and records `productPasses:0`. The controlled transport is a stand-in: this pass
qualifies bytes and mode handling only. The real subprocess/driver test checks
the adapter's mode reads and input writes across all six resizes; it is a protocol
fixture, not a Rust PTY product journey or kernel-resize proof.

For the later declared product run, set `inputProfile` to `terminal-only` or
`example-ghostty-keybinds` in an existing `capture-run.ts` plan. Both profiles are
required at S9, in legacy and kitty modes. Each key/text step then uses the new
adapter; omission retains the original behavior. A consumed key stops that
journey with an input prerequisite finding, rather than waiting for an impossible
frame or reporting a product red. `input-delivery.json` retains key, actual mode
snapshot, route and bytes or consumed reason even when later checkpoints fail.
These are PTY-write acknowledgements, not input-consumption or effect receipts.

## Pointer matrix at S9

`GhosttyJourneyInput.click` sends left press **and release**; `mouse` sends wheel
up/down. SGR1006 uses one-based cell coordinates, M for press/wheel and m for
release, wheel codes64/65. It reads actual tracking mode0/1000/1002/1003 and wire
format for each operation. Mode0 consumes without writing; non-SGR and pixel
formats return an explicit unsupported prerequisite. It never enables modes.
Coordinates outside the current viewport reject. Primary implementation:
[Ghostty Surface.zig:3600–3780](https://github.com/ghostty-org/ghostty/blob/v1.3.1/src/Surface.zig#L3600).

`layer-c-harness-journeys.ts` authors the Sessions and Quota pointer matrix:
120×36, 76×30, 48×24, 48×12, 40×24, 32×24; both profiles/key modes; clicks,
release and both wheel directions before/after **same-child kernel resize**,
inside controls and at targets beneath dialog/menu/dropdown overlays. Recompute
coordinates from each actual grid. Require completed product redraw and retained
identity after resize. Mode readback must come from that child's PTY output.

Bind those semantic steps after S6/S7. Overlay no-change claims additionally
need the [adopted causal input acknowledgement](input-consumption-ack-proposal.md):
sequence, actual surface, target/action or null, and normalized event with no
text/paste contents. Its test-only InputOwner producer is assigned to S6; the
Verifier reader waits for real producer bytes. A PTY write ACK or no new frame
is insufficient. Assert dialog outside-click behavior separately from menu/dropdown
close-and-no-click-through. The common byte adapter does not prove those effects.

## Remaining limits

The final observation report must retain these unverified classes: physical
keyboard hardware, Ghostty app keybinds beyond the modelled set, OS-level
shortcuts, and real Ghostty rendering beyond libghostty. Also retain any S9
product/mouse-effect, input-consumption or kernel-resize gap that remains open.
S5 wrong-build verification still takes priority when its target is declared.
