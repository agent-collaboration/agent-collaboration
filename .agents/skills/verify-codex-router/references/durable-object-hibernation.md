# Durable Object hibernation

Use this lasting journey for the central link's local workerd forced-eviction boundary. The driver is `verification/observations/central/durable-object-hibernation.ts`; its `--help` describes inputs, evidence and limits. Its real workerd stand-ins qualify the observer before C2 exists. **The C2 product journey remains draft:** neither a fixture pass nor the old SDK session fixtures establish that CentralObject uses the new link correctly.

The adopted outcome is Lead's C disposition of 2026-11-15: fixed text `ping` / `pong` handled by platform auto-response; Router alone schedules liveness; a hibernating socket survives eviction; the next real frame reconstructs the object and restores connection identity. Re-read live enrollment on connect, renewal and after wake. Attachments carry identity, never perpetual authorization. Lead's verification-maintenance commission requires the non-hibernating control to fail first. The governing workspace-storage design and current owner decisions remain the source for the C2 semantic request and authorization result.

## Stand-in qualification

Run from the repository root with Node and the lockfile-pinned tooling installed:

```sh
pnpm --dir verification install --frozen-lockfile
node verification/observations/central/durable-object-hibernation.ts --help
mkdir -p tmp/do-hibernation
node verification/observations/central/durable-object-hibernation.ts --fixture non-hibernating --evidence tmp/do-hibernation/non-hibernating
node verification/observations/central/durable-object-hibernation.ts --fixture hibernating --evidence tmp/do-hibernation/hibernating
```

Use fresh evidence directories on every run. The first command that drives a fixture **must exit 1 with `disposition.kind: "expected-red"`**. The driver accepts only the complete stage prefix through `eviction-requested`, the pinned active-references eviction error, and `disposed: true`. Any other negative-control outcome is `invalid-control`, exit 2. A startup, missing dependency, failed cleanup or malformed fixture is not the required red. The negative stops before the deciding frames; the pure assertion tests cover their forbidden cases. The second fixture must exit 0 with `disposition.kind: "passed"`, every stage through `same-socket-new-instance-restored-attachment-and-store`, `failure: null` and `disposed: true`.

Each run creates one Miniflare instance with pinned Miniflare `5.20261012.0-beta` and workerd `1.20261012.1`, an ephemeral loopback listener and one SQLite-backed test object. `runtime.ready` and an actual 101 upgrade establish readiness; Welcome and a real frame are the read-only/functional health check. The fixture's `observe` frame increments its private counter to prove that the same owning SQLite state is used before and after eviction. It is test support, not a product command.

The deciding sequence is:

1. Generate a fresh UUIDv7 per connection and send it only in the upgrade's `x-hibernation-attachment` header. The object stores it through `serializeAttachment`; it is absent from environment bindings. Upgrade once and retain that exact client socket. Capture Welcome's instance and attachment witness, then observe counter 1. A fresh object cannot re-derive this witness from env or a second upgrade.
2. Call `unsafeEvictDurableObject("hibernation-witness", "HibernationWitness", {name: objectName, webSockets: "hibernate"})`. The pinned API forwards this to `workerd:unsafe.evict` for the exact stub. Do not dispose/restart the runtime or open a new socket.
3. Send three literal `ping` frames on the original connection. Require literal `pong` each time and unchanged observed constructor/handler counts. Record each constructor's UUIDv7 instance and the Node observer phase in which its witness arrives. The witness service lives outside the target object, so reading its counts does not wake that object.
4. Set phase `deciding` immediately before dispatching the next real `observe` frame. Require a different UUIDv7 instance, the identical attachment, counter 2, exactly one additional constructor witness and handler, no replacement Welcome, and the serving instance's constructor witness arriving in phase `deciding`. A witness arriving in `heartbeat` cannot be credited to this frame.
5. Snapshot meaningful counts and constructor witnesses before settlement. Close the retained socket and await Miniflare disposal, including when the signal wrapper disposes and then rejects construction. `disposed` tracks actual completed disposal; `postSettlementCounts` are informational because close itself may wake the object. The standalone entry uses the established owned signal-shutdown wrapper; an in-process caller must keep its existing Miniflare signal policy. No Rust executable is launched; any future Rust/build entry must use the repository's signed runner. A stalled frame has no timer: end that exact recorded run with SIGINT or SIGTERM and await graceful disposal (exit 130/143). Such interruption is not a passing receipt. Never SIGKILL a bridge or workerd.

Keep raw `receipt.json`, command and exit code, source head, `dirtyTree`, driver/fixture/signal-shutdown hashes and package versions in the task's evidence. Preserve the first red; do not overwrite it with a rerun. Receipts include decoded Welcome/before/after frames, heartbeat counts, constructor phases and machine-classified disposition. Unit tests pin error messages for unchanged-instance, lost-attachment, reset or extra counter increments, heartbeat/eviction witnesses, missing or extra deciding constructors/handlers, replacement Welcome and wrong constructor phase. They also reject invalid negative controls and exercise wrapper disposal before rejection.

The driver uses explicit frames and I/O completion, with no wall-clock reads or sleeps and no scheduled heartbeat. This checks auto-response behavior, not 20-second client scheduling. That scheduling must use the product's one injected clock in C1/C3. Miniflare's internal bounded eviction failure is a platform diagnostic, not an assertion about elapsed time.

## Compiled CentralObject adapter

The existing `verification/targets/workerd-storage.ts` loader accepts an optional
`C2WitnessRecorder` on its bundle. It inserts a verification-only module around
the original compiled `CentralObject`, preserves its namespace and methods, and
records witnesses in an external Node service. `c2-witness.ts` defines the strict
event/attachment schemas and `assertC2Restore`; the closed attachment carries
identity plus the original effective deadline, never a bearer or permissions.
The product hooks must be built with non-default `test-support`. Missing hooks,
failed acknowledgements or malformed events invalidate the observation.

The existing Rust-owned bridge opts in with `--c2-witness` alongside its sign-in
plan and injected `--launcher-time-ms`. Its JSON-lines control interface is:

- `{c2Witness:{phase}}` returns the external event records and sets the next
  recording phase: `connecting`, `baseline`, `evicting`, `heartbeat`, `deciding`,
  or `settling`. Each record has sequence, phase, instanceId and the event.
- `{c2Evict:organizationUuid}` evicts only that organization's `CentralObject`
  in the original loader using `webSockets:"hibernate"`.
- `{setExternalTimeMs:milliseconds}` updates only the existing external clock
  cell. It sends no target HTTP request. Existing `setTimeMs` pushes time into
  live objects and therefore wakes them; retain its established semantics.

These controls refuse without opt-in. Recorder snapshots never fetch the target.
Actual live SQL read events must precede the matching restoration result; repeated
reads during a held admission recheck are allowed. Alarm snapshots contain the
actual owning `EARLIEST_DUE` result and platform `getAlarm`, and must agree.
Neither a generic admitted flag nor matching synthetic values supplies those reads.
The permanent wrapper qualification stand-in proves loader/delegation behavior
only; it is not a compiled CentralObject or an authentication oracle.

## C2 integration prerequisite

Before claiming C2 coverage, declare its frozen source, compiled bundle/module hashes and local allocation. Commission apparatus upkeep to connect this oracle to that exact compiled CentralObject through the existing workerd conformance loader. The C2 Implementer owns any product-side test-support seam needed to expose:

- constructor and message-handler witnesses observable without fetching the target object;
- restored socket-attachment identity and a fresh live-enrollment read;
- a real semantic Request/Reply and its owning workspace-store result;
- the existing EARLIEST_DUE alarm readback, NULL when no due work exists and unchanged by heartbeat traffic.

Do not add a test-only production route, bypass admission with an asserted person, or substitute a synthetic counter for that semantic request. The fixture protocol here is not a selected product wire contract. Maintain strict decoded witnesses and the same original socket. Include revoked/expired/superseded enrollment after wake. Keep a real non-hibernating control against the identical deciding oracle. No target HTTP inspection may run between eviction and the deciding frame, since that would cause constructor re-entry before the frame.

Real admin revoke/supersede calls wake the object: perform them before the final
eviction in those denial cases, retain their witnesses, then require fresh live
denial after reconstruction. For while-evicted expiry, use only the external clock
update. During the protected interval permit only recorder snapshots and the
original socket's fixed heartbeat; an unrelated alarm invalidates isolated wake
attribution. The deciding request is a correlated `CreateArea`, followed afterward
by a real read request and an independent read of the owning workspace SQLite row.
The permanent Rust socket journey owns provisioning, token signing, semantic
encoding and row assertions. Its exact declared compiled selector and sanitized
receipt handoff are still prerequisites; do not replace them with the stand-in.

## Limits

The heartbeat result is **no observed constructor witnesses or handlers through each pong**, plus the serving constructor witness arriving only after `observe` dispatch. Handler witnesses are ordered before their reply. Constructor witnesses are external callbacks: an arbitrarily delayed constructor-only wake triggered earlier can arrive during `deciding`. Phase attribution cannot prove when such a constructor began. This journey therefore does not claim absolute zero constructor executions caused by heartbeat traffic; that stronger causal claim remains unverified.

Local forced eviction proves only the selected runtime's eviction/socket/attachment semantics. It does not establish natural idle hibernation, absence of resident tasks in C2, deployed Cloudflare behavior, idle timing or billing. Inspect the real C2 path for `setTimeout`, `setInterval`, unfinished I/O, `waitUntil`, ordinary accepted sockets and event-stream tasks separately; a source grep alone is not a hibernation pass. Hosted observations need their own declared target and authorization. Host caller stamping, CLI/MCP tools, real machine-token admission, D provider handoff/minute batching, release and owner acceptance remain separate proof layers.
