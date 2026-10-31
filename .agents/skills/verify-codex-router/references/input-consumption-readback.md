# Input consumption reader and returned picker outcomes

Reader qualification is separate from product observation. The input serializer
was inspected at `9bcab4fff50aa5b6b0421a3f5dfdc57e38a21fb5`, including
InputOwner's publication after `route_event` and `on_routed`. The Sunclaw handoff
is `codex-router.feat-tui-s6-picker/tmp/worker/tui-s6-picker/input-receipt-handoff.md`.
S6b fixture forwarding and actual produced picker bytes remain unbound.

## Acknowledgements

`scripts/verification/tui/sessions/input-consumption-schema.ts` models the exact five
root fields: sequence/event/surface/target/action. Events use the producer's
nested key and mouse tags. Text and paste have no payload fields; modifier arrays
are unique and in producer order. Overlay IDs stay canonical decimal u64 strings;
sequence is a positive JSON-safe integer. Unknown fields and malformed values
reject. Fixed key names remain the serializer's underlying KeyCode Display
strings, not an invented independent enum.

`InputConsumptionReadback.open(path)` in `input-consumption-readback.ts` binds
one explicit, owned, non-symlink `input-dispatch.jsonl` file. Initial complete
records are retained as a baseline, with sequence starting at one; they are not
claimed to be verified input effects. The instance never re-baselines. File
replacement, byte-prefix rewrite, truncation, missing/duplicate/gapped/reordered
sequence and any unaccounted record fail. Reusing another child's file or
concatenating runs cannot supply a valid observation.

```ts
const reader = InputConsumptionReadback.open(receiptPath);
const records = await reader.observeInput({
  expected: independentlyDeclaredEventsAndRoutes,
  inject: () => retainedDriver.send(exactBytes),
  timeoutMs: 8000,
});
// Assert independent model/selection/query/draft and effect counters here.
// Then observe a completed real frame; an ack is not a redraw.
await retainedDriver.finish(); // real fixture must also flush_and_check successfully
reader.assertComplete();
```

The observer arms a watcher on the already-created owned file before injecting and reads
complete JSON lines on notifications and once injection completes. The timeout
is only a failure guard, not a latency oracle. No polling, sleep, quiet period or
redraw heuristic supplies success. Partial lines can wait while the producer is
active; they cannot pass a final read or establish an input baseline. Every
preceding Resize or other decoded event must be declared explicitly in the
expected list; nothing is silently skipped. A click that remains mounted has
press/release expectations. A terminal action has only events actually decoded
before unmount, never an imagined release.

Matching is exact for normalized event, old owning surface, consumer target and
invoked action. Caller expectations must come from the adopted scenario and
independent fixture state, never from the receipt being checked. An outside
press can name the closed old menu; its later release can name the screen.
Dropdown-primary and Ctrl+C exceptions retain their actual parent/screen route.

A live match establishes only a **prefix**. A later extra acknowledgement is
caught by the next observation or `assertComplete`, which must run after actual
fixture completion and successful sink flush. There is no sleep-based attempt
to predict whether future bytes will arrive. Failure latches and prevents silent
resynchronization or another injection. Abort removes the watcher/timer; it does
not claim to cancel an injection already delivered to the product.

This file has no PID/source/build fields. Bind its nominated directory to the
same retained child and source/artifact receipts in the existing capture route.
The reader's inode/prefix check is continuity evidence, not authentication of
an arbitrary writer. Preserve raw bytes and independent state/effect evidence.

## Returned outcomes and U5 source split

Lead pushes `01a18e33-0249-7fa0-9756-eeebb1a22d1c` and
`01a18e37-0d51-78d2-b22c-01996654eb1d` adopt:

```json
{"kind":"new","request":{"client":"claudeCode","machine":"localCodex","directory":"/fixture/project","runMode":"backgroundThenAttach"}}
```

The closed client values are codex/claudeCode, machine is defaultHosted,
localCodex or configured:<validated-name>, and runMode is foreground or
backgroundThenAttach. U4 keeps cancelled, resume and fork with the existing
source-qualified identity variants.

`source-qualified-outcome.ts` parses one JSON object, rejects zero/multiple
outcomes, compares all fields to an independently captured request/identity,
and never falls back to a terminal marker. `checkLocalNewOutcome` accepts the
local/default request as U5's Start-once proof without requiring a separate
preparer count. It rejects configured requests: those retain the distinct
`preparation-readback.ts` counted stand-in, with exact count/request. A generic
configured outcome parse is not configured dispatch cardinality proof.

The nine New journey drafts cover three activation routes (kitty compound,
legacy Tab/Enter, click) across default/local/configured sources. Local/default
use the source-qualified-outcome port; configured uses preparation. Wrong
client/run mode, missing/duplicate outcome and wrong origin/service/endpoint
controls are reader checks, not evidence that a picker produced those bytes.
The previous S0 `input-journeys.ts` remains a historical pre-UI baseline: its safe
marker and pending preparation step do not satisfy the new U5 oracle. Bind the
new reader/ports only after the S6b fixture entry and exact target are declared.

## Qualification and pending integration

```sh
pnpm --dir scripts/verification/tui exec vitest run input-consumption-readback.test.ts input-consumption-file.test.ts source-qualified-outcome.test.ts new-start-oracle.test.ts
node scripts/verification/tui/journeys/flows/flow-catalogue.ts --check
```

Literal wire fixtures and controlled real-filesystem writers qualify schema,
correlation, continuity and failure sensitivity only. Required real producer
controls remain: wrong surface/action/event, dropped/duplicate ack, false
post-close surface, independent covered-state/effect checks, and normal/release
producer omission. S6b real-byte integration and S9 product passes remain open.

## Native watcher qualification limit

On this macOS Node 26.12.1/libuv 1.53 host, an immediate same-turn synchronous
write after fs.watch creation could be missed even under an 8-second diagnostic
guard. A separate controlled process produced a native event in 64 ms. The
[libuv implementation](https://github.com/libuv/libuv/blob/v1.53.0/src/unix/fsevents.c)
queues CF stream rescheduling asynchronously in uv__fsevents_init. The permanent
notification control uses a separate producer, matching the process boundary;
its empty post-injection read is observed through a passthrough IO fact, not a
sleep or call-count assertion. Actual S6b injection timing remains unqualified.
A missed wake is an observer-readiness gap, never a product failure. No polling,
extra timeout padding or alternate engine was added.

U4 return-path drafts now use same-ID HostedCodex rows across service and endpoint
variants, per Lead push 01a18e4a-bb03-7553-bb56-b784c5ad45e3. HostedProvider Open
uses the in-picker client-action route and does not return ResumeSession. Its
component-test proof remains separate; this checkpoint does not rerun it.

## S6 journey integration rebaselines

Lead push `01a18e55-989c-7ba1-a6d5-f399853c8f02` adopts four changes:

1. `baseline-journeys.ts` observes main Esc through a real Help open/close
   processing barrier, compares the restored main text/selection, and then sends
   Ctrl+C and requires the child's successful exit and single cancellation outcome.
   It never waits for Esc to terminate the picker. The Help round trip contributes
   to this observation; input-dispatch route proof remains a separate U1 oracle.
2. Picker Help uses the shared **Help** title and its Sessions context. The Esc
   convention must say **never quit**; the historical unconditional defect row is
   replaced by that observed convention, with main Esc checked separately.
3. `input-journeys.ts` requires an independently declared `newRequest` and creates
   a fresh `session-receipts` directory per New run. After successful child exit it
   reads exactly one `picker-outcome.json` and compares the full returned request.
   No local/default preparer count is required or invented. Both journey plans
   accept `pickerOnly: true`; input journeys also accept `--picker-only`.
4. `PtyGridTarget.storyboardVariant` and the Rust bridge admit
   `source-qualified-outcome` and `configured-source-new`, preserving the existing
   `wide-glyph-left-border` variant. All three remain sessions-filters only in the
   Rust owner; no alternate PTY or synthetic product frame is introduced.

Example input-plan additions to the existing source/artifact-bound plan:

```json
{"pickerOnly":true,"newRequest":{"client":"codex","machine":"defaultHosted","directory":"/repo/project-a","runMode":"foreground"}}
```

The directory is the fixture's captured path, not the verifier checkout's cwd.
Old `preparationProof` input plans must be replaced with this request declaration.
Configured New uses the counted `source-preparation.json` readback with the same
request wire (`codex`/`claudeCode`, `configured:<name>`, foreground/backgroundThenAttach).
A returned New outcome cannot substitute for that configured preparer count.

Controlled bridge tests qualify these observer changes in both key modes and
reject changed main state; they are not product passes. S6b must merge this head,
rebuild/sign its declared fixture and driver, retain source/artifact bindings,
and run the actual picker journeys. S9's authored coverage is still not a run.

### S6b first-run observer corrections

Lead pushes `01a18e73-a614-7360-86de-8e72306008bf`,
`01a18e74-e256-7ef0-80bb-e31bd79c7a35` and the source correction
`01a18e76-00ff-74a3-b5cb-f25513a72d09` refine two apparatus steps:

- On the S6 picker path, nominate receipts before entry. Send Esc alone and await
  its exact decoded Press acknowledgement: screen, null target, action `escape`.
  A bound action remains the recorded action when its handler changes nothing.
  Then send F1 separately. Legacy bytes `1b 1b 4f 50` in one write are ambiguous
  with Alt+F1; a transport OK, a sleep or a stale main frame cannot separate them.
  Both key modes use this barrier. The subsequent F1, first Help overlay's Esc,
  and Ctrl+C also have exact acknowledgements; after fixture exit, assert the
  complete stream. Keep the Help round trip and unchanged main-grid comparison
  as independent state/frame proof. Quota's producer remains an S7 prerequisite;
  this correction does not qualify its retained baseline path.
- `input-journeys.ts::targetCell` requires the Button's Focused or Primary role,
  or its actual primary-button actions row, within the active dialog bounds.
  The primary role is the shipped Label RGB `#9cbcfb`; Focused is the qualified
  terminal palette's bright yellow. A generated `Ctrl+Enter Start` hint is not
  a second button. Hint-only and duplicate real-role scenes must reject.

Controlled VT/bridge tests qualify the observer and strict final counts. A replay
against S6b's retained New grid located the real Start at zero-based (34,20),
rejecting the hint at (28,21). This is retained-grid replay, not a fresh product
journey or a physical-terminal input claim. Deliberate unambiguous input bursts
in other journeys remain unchanged.

### Focused Help body acknowledgement

Lead push `01a18ec2-f6e6-7f71-abc6-1a2588048578` supplies the actual S6b
main-Esc receipt: the third event is Esc Press, overlay `1`, null action,
target `11:action:help:help-body`. `HelpControl::body_target()` constructs the
action target's `help-body` child. The observer derives the UTF-8 parent byte
length by the same `TargetId::child` rule. It no longer expects a null target
for a key consumed by that focused Help body. The other fields, four-event
final count, Help round trip and unchanged-grid assertion remain exact.
Controlled bridge cases in both modes must reject the stale null target.

### Post-measurement entry and generated Help length

Lead push `01a18ec9-d0c5-7281-ad4d-dce399064080` corrects S6 component
entry settlement and Help scrolling. On the `pickerOnly` Sessions path, the
first draw measures the list and the next completed draw paints its scrollbar
focus marker. The before snapshot therefore requires received completed frame
ordinal 2 or later, not the first title-bearing frame or an arbitrary next
frame. Record `entry-frame-binding.json`, and cross-check the parser generation
against the actual synchronized-output end markers in retained PTY bytes.
This works when frames arrive separately or in one chunk; elapsed time cannot
satisfy it. Keep the entire before/after grid comparison, including query,
selection, title and the scrollbar marker. The retained legacy and kitty
`s0-baseline-help-ack-current` frame-2 grids match the corresponding restored
main grids; the explicitly unvalidated sibling run is not evidence.

Help generates Actions above Input conventions, so its scroll distance is not
a fixed count of Down keys. Send End and require a completed Help frame in which
the literal `never quit` row is visible. A missing row remains an observation
failure. Controlled tests retain changed-grid, null-target and missing-legend
negative cases, as well as exact four-event final ACK counts.

### Details action label follows the row identity

Lead push `01a18efe-6047-77f3-b070-704c1eb77225` corrects the over-broad
legacy Details fallback. The declared initial fixture row is native Codex
Alpha, so its resume action is `Resume`, not `Open`. Native Claude Details
projects that action as `Open` (including the running native identity in the
current product); managed provider actions remain source-capability qualified.
The fallback chooses by the declared row kind and retains the exact single
`resume:00000000-0000-4000-8000-000000000011` marker assertion. It does not
infer an expected action from whichever button happens to be drawn. Controlled
button-role grids reject the opposite row's label and missing Fork actions.

### Measured Details and a bounded exit drain

Lead push `01a18f06-903c-7bb2-99be-2ff6b9ced3d7` requires the Details
fallback to observe Enter's exact screen/details acknowledgement before choosing
a click target. Replay actual received synchronized frames individually to locate
the first Details paint, then require its subsequent completed measurement redraw.
A pending main redraw does not count, and coalesced first/second dialog frames
must work. Retain `details-frame-binding.json` with the acknowledgement and frame
ordinals. The click comes from the latest settled grid, not the first title match.

The input journey exit drain uses one 8-second failure deadline across all reads,
including a live child that keeps producing bytes. Only PTY EOF followed by the
Rust driver's successful child-exit/finish check permits an outcome assertion.
Missing EOF or read failure throws an apparatus error; neither becomes a product
pass or red. The enclosing retained driver's stop path still owns cleanup.

Lead push `01a18f0c-b921-7222-8244-b724532935ef` corrects that Enter
acknowledgement's target to `sessions-list`. `InputOwner::route_key` delivers to
the focused enabled target first: consumed input and an unmodified Enter
activation retain that target; action-registry shortcuts, primary Ctrl+Enter,
global Quit and ignored events record null targets. Existing main Esc/F1/Quit
expectations stay null under those actual routes, while Help-body Esc retains
its consumed body target. Do not blanket-copy focus into every key receipt.
The controlled Details bridge now uses the retained producer's list target and
rejects the stale null-target record without relaxing event/surface/action.
