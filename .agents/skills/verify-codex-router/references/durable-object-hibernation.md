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

Use fresh evidence directories on every run. The first command that drives a fixture **must exit 1** after readiness, Welcome and the initial SQLite-backed frame, at `eviction-requested`. Its receipt must identify an eviction failure caused by active references, with `disposed: true`. A startup, missing dependency or malformed fixture failure is not the required red. The second must exit 0, with every stage through `same-socket-new-instance-restored-attachment-and-store`, `failure: null` and `disposed: true`.

Each run creates one Miniflare instance with pinned Miniflare `5.20261012.0-beta` and workerd `1.20261012.1`, an ephemeral loopback listener and one SQLite-backed test object. `runtime.ready` and an actual 101 upgrade establish readiness; Welcome and a real frame are the read-only/functional health check. The fixture's `observe` frame increments its private counter to prove that the same owning SQLite state is used before and after eviction. It is test support, not a product command.

The deciding sequence is:

1. Upgrade once and retain that exact client socket. Capture Welcome's instance and UUIDv7 attachment witness, then observe counter 1.
2. Call `unsafeEvictDurableObject("hibernation-witness", "HibernationWitness", {name: objectName, webSockets: "hibernate"})`. The pinned API forwards this to `workerd:unsafe.evict` for the exact stub. Do not dispose/restart the runtime or open a new socket.
3. Send three literal `ping` frames on the original connection. Require literal `pong` each time and unchanged external constructor/handler counts. The witness service lives outside the target object, so reading its counts does not wake that object.
4. Send the next real `observe` frame. Require a different UUIDv7 instance, the identical attachment, counter 2, exactly one new constructor and handler, and no replacement Welcome.
5. Close the retained socket and await Miniflare disposal, including on failures. Never SIGKILL a bridge. The standalone entry uses the established owned signal-shutdown wrapper; an in-process caller must keep its existing Miniflare signal policy. No Rust executable is launched; any future Rust/build entry must use the repository's signed runner.

Keep raw `receipt.json`, command and exit code, source head, changed driver/fixture hashes and package versions in the task's evidence. Preserve the first red; do not overwrite it with a rerun. Receipts include decoded Welcome/before/after frames and heartbeat counts, not only a final pass label. Unit tests exercise the observer's forbidden unchanged-instance, lost-attachment, reset-counter and heartbeat-wake cases. Those unit tests supplement the real negative control.

The driver uses explicit frames and I/O completion, with no wall-clock reads or sleeps and no scheduled heartbeat. This checks auto-response behavior, not 20-second client scheduling. That scheduling must use the product's one injected clock in C1/C3. Miniflare's internal bounded eviction failure is a platform diagnostic, not an assertion about elapsed time.

## C2 integration prerequisite

Before claiming C2 coverage, declare its frozen source, compiled bundle/module hashes and local allocation. Commission apparatus upkeep to connect this oracle to that exact compiled CentralObject through the existing workerd conformance loader. The C2 Implementer owns any product-side test-support seam needed to expose:

- constructor and message-handler witnesses observable without fetching the target object;
- restored socket-attachment identity and a fresh live-enrollment read;
- a real semantic Request/Reply and its owning workspace-store result;
- the existing EARLIEST_DUE alarm readback, NULL when no due work exists and unchanged by heartbeat traffic.

Do not add a test-only production route, bypass admission with an asserted person, or substitute a synthetic counter for that semantic request. The fixture protocol here is not a selected product wire contract. Maintain strict decoded witnesses and the same original socket. Include revoked/expired/superseded enrollment after wake. Keep a real non-hibernating control against the identical deciding oracle. No target HTTP inspection may run between eviction and the deciding frame, since that would cause constructor re-entry before the frame.

## Limits

Local forced eviction proves only the selected runtime's eviction/socket/attachment semantics. It does not establish natural idle hibernation, absence of resident tasks in C2, deployed Cloudflare behavior, idle timing or billing. Inspect the real C2 path for `setTimeout`, `setInterval`, unfinished I/O, `waitUntil`, ordinary accepted sockets and event-stream tasks separately; a source grep alone is not a hibernation pass. Hosted observations need their own declared target and authorization. Host caller stamping, CLI/MCP tools, real machine-token admission, D provider handoff/minute batching, release and owner acceptance remain separate proof layers.
