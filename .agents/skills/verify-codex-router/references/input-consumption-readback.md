# Input consumption reader and returned picker outcomes

Reader qualification is separate from product observation. The input serializer
was inspected at `9bcab4fff50aa5b6b0421a3f5dfdc57e38a21fb5`, including
InputOwner's publication after `route_event` and `on_routed`. The Sunclaw handoff
is `codex-router.feat-tui-s6-picker/tmp/worker/tui-s6-picker/input-receipt-handoff.md`.
S6b fixture forwarding and actual produced picker bytes remain unbound.

## Acknowledgements

`scripts/verification/tui/input-consumption-schema.ts` models the exact five
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
node scripts/verification/tui/s9-prestage.ts --check
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
