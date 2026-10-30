# Adopted S6 causal input acknowledgement

**Adopted by Lead push `01a18d2a-8b01-7c03-b2c3-f7b720df0252`, including
the fifth field `event`; no product producer or reader is added here.** Grounded in
published InputOwner source at `53df30c4fb274315c47806080958ab8dff5a7ca0`,
inspected for this proposal; that read is not a declared S4 observation target.

## Home and lifetime

Producer DTO/sink:
`crates/agent-collaboration-tui/src/input_dispatch_receipt.rs`, gated entirely by
the verification `test-support` feature. The real
`input_owner.rs::InputOwner` terminal-event closure owns publication, after
`route_event` and its `on_routed` callback have returned. It writes one complete,
flushed JSON line to `input-dispatch.jsonl` in the harness's existing owned
receipt directory. No normal/release producer, sink, environment switch or
terminal subscription; no extra event injection path.

The eventual reader belongs at
`scripts/verification/tui/input-consumption-readback.ts`. Bind it to the same
retained child and capture source/build receipt. A fresh process uses a fresh
file; never accept another child's receipt, concatenate runs or overwrite old
evidence. The writer is per InputOwner, not process-global.

## Fields

```ts
type SurfaceReadback =
  | { kind: 'screen' }
  | { kind: 'overlay'; overlayId: string }; // actual OverlayId decimal u64

type InputDispatchReceipt = {
  sequence: number; // starts at 1; contiguous, checked JSON-safe integer
  event: InputEventReadback;
  surface: SurfaceReadback;
  target: string | null; // actual consuming/activating TargetId, or none
  action: string | null; // actual invoked ActionId, or none
};
```

The four requested facts are sequence, surface, target and action. **The adopted
additional correlation field, `event`, is needed:** a sequence alone cannot
distinguish the tested click from an intervening Resize or ignored release.
`InputEventReadback` is a narrow projection of the actual delivered TerminalEvent:
Key(code, modifiers, press/repeat/release), Mouse(kind, button when applicable,
column, row), Resize(columns, rows), or Other(kind only). Do not record paste or
text-field contents. Printable Char input becomes a payload-free `text` category,
not its character/code point; paste likewise retains only its kind. Preserve
code identity for fixed non-text keys and the supported modified control
shortcuts. Derive those names from the existing typed event, never the expected
test. Text redaction cannot turn a Key or Resize into the awaited Mouse event.
The Implementer builds the producer in S6; the Verifier writes the reader against
those real S6 bytes, preserving the controls below.

`SurfaceId::{Screen,Overlay}` supplies the surface; OverlayId::get supplies its
identity. TargetId::as_str and ActionId::as_str supply their validated IDs. Null
means none, never an empty string or the literal string "none". No new semantic
IDs, titles, selected-session IDs or credentials are needed. Retain u64 overlay
IDs as decimal strings, rather than losing them through a JSON number.

## What the producer must report

Capture the actual routing decision as it happens. Do not infer it afterward
from current focus, the new top surface, a screen title or Routed::Ignored:

- Normal unused/outside input reports the top surface that owned/discarded it,
  with null target/action. An event still earns an acknowledgement when ignored.
- A target that returns Used/Scrolled, or whose activation is invoked, reports
  that actual target. A focused target returning Unused is not falsely reported
  as the consumer if routing continues elsewhere.
- A scope key action reports its actual surface and invoked ActionId. Focus
  traversal itself has null action/target; focus outcome is observed separately.
- An outside menu/dropdown mouse press reports the old overlay, even after it
  closes. It must not relabel that press with the newly exposed screen. The
  later release routes against the then-current top surface and must not
  activate the underlying target; R1 still handles each event separately.
- Ctrl+C reports the screen action even with an overlay open. Ctrl+Enter beneath
  its own dropdown reports the parent dialog/primary action, not the popped
  dropdown or later screen. These are the two permitted R1 exceptions.
- Resize, ignored key repeats/releases and unhandled events also advance the
  sequence, with their actual event identity, so unrelated input cannot satisfy
  the awaited acknowledgement.

The source currently returns Action, Text, OverlayOpened/Closed, Quit or Ignored.
Those values alone lose the route's surface and sometimes target. Recording only
the final Routed enum would reproduce the gap. Keep routing/handling real; the
test-only instrumentation observes its chosen branch without replacing it.

## Reader proof and limits

Read the last complete sequence, inject one declared input event, then await the
next complete line matching the actual normalized event. Unexpected preceding
events must be identified and accounted for, never silently counted as the
tested click. A non-terminating click has press and release records; each wheel
event has one. Count actual decoded events while InputOwner is mounted, not a
release that never reaches it after a terminal action has ended the loop.
Truncated/missing lines, duplicate/gapped/reordered sequence numbers, foreign
surface/target/action or a mismatched event cannot pass. Use a bounded file-event
wait plus a complete-line read, not a sleep or a redraw heuristic.

An acknowledgement establishes **synchronous routing completion**, not a redraw,
async task settlement, persistence or absence of arbitrary hidden effects.
After the exact ack, assert the independent captured model/selection/query/draft
and effect counters; then use a completed real frame (or close the overlay and
inspect the underlying screen) for rendered outcome. Keep async operation/job
receipts separate. A no-change assertion without the matching ack remains open.

Required controls: deliberately route a covered click to the wrong surface;
fire an unexpected action; match a Resize to a click; drop/duplicate an ack;
close an overlay then misreport the newly exposed surface. Each must fail the
real producer/reader and independent state/effect check. A handwritten JSON test
qualifies only reader sensitivity. Normal/release builds must be shown to omit
the feature and producer entirely. The Implementer owns these S6 changes.
