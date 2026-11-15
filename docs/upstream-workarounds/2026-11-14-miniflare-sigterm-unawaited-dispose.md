# Miniflare signal exit does not await workerd disposal

Standalone verification bridges must finish disposing their workerd child before exiting. With pinned Miniflare `5.20261012.0-beta`, its signal exit hook does not establish that ordering. This workaround is verification-only; it changes neither shipped Workers nor Router process management.

The installed `verification/node_modules/miniflare/dist/src/index.js` contains the decisive source:

- Lines 89518–89552: signal handlers invoke callbacks and immediately call `process.exit(128 + signal)`.
- Lines 133472–133479: the constructor registers a callback that calls `void this.#runtime?.dispose()`.
- Lines 117167–117180: runtime disposal kills workerd and returns its process-exit promise.
- Line 135604: ordinary awaited Miniflare disposal removes the exit hook and waits for runtime cleanup.

Node's `EventEmitter.emit` invokes listeners in order without awaiting their returned promises. Registering an async bridge handler before Miniflare's handler cannot prevent the latter from terminating the bridge while disposal remains pending.

Hosted runs `43656799717` (timing head `3e66ad2d7`) and `43658134393` (B5 head `2b8d029875`) failed the launcher's immediate OS exit assertion after its command-error assertion passed. The logs name a surviving PID, but do not establish whether it was live or a zombie. Ten local main repetitions and ten local timing-correction repetitions passed on macOS; that does not dismiss the Linux failures or establish their exact PID state.

## Owning boundary

`verification/test-support/miniflare-signal-shutdown.ts` brackets synchronous construction inside a standalone bridge. It snapshots `SIGINT`, `SIGTERM`, and `SIGHUP` listeners, removes only listeners that construction added, then installs handlers that await Miniflare disposal before exiting with the signal's conventional code. A missing expected added hook refuses startup after disposing the constructed runtime, so a changed upstream hook cannot silently restore the unsafe path. The ordinary `exit` listener remains for Miniflare to remove during disposal.

The modern bridge supplies this factory through the shared loader's optional constructor dependency. The legacy standalone script uses the same factory directly. The shared `WorkerdStorage.start` carries no signal policy: direct in-process TypeScript callers retain their existing behavior and may own several concurrent Miniflare instances. The listener-difference guard applies only to the standalone bridge's one constructor, not every runtime in a shared process.

Ordinary fixture cleanup closes stdin and awaits process exit. The launcher command-error test closes its existing command input to exercise the error-and-settlement path; it does not use an unawaited upstream signal exit as its fault mechanism.

## Reproduce and verify

After building the declared support bundles and installing verification dependencies, run from the repository root:

```sh
node --test --test-name-pattern='SIGTERM awaits owned disposal' verification/targets/workerd-bridge-lifecycle.test.ts
cargo nextest run -p collaboration-central-cloudflare --features test-support --test integration -E 'test(launcher_command_error_settles_real_bridge_before_returning)'
```

The permanent real-SIGTERM test holds disposal at an explicit stdin gate, checks that the bridge stays alive until release, then waits for bridge exit and requires its recorded workerd PID to return `ESRCH`. It also checks that a pre-existing listener remains available. There are no sleeps, polling loops, retries, or increased deadlines. A separate permanent guard test requires disposal before refusal when the expected upstream listeners are absent.

Only inspect the recorded owned PID. A broad `pgrep -x workerd` can identify residue in an isolated CI job, but is not sufficient attribution on a development machine running unrelated verification.

## Removal check

Remove this workaround only after a newly pinned Miniflare version's signal path awaits runtime disposal and its process-exit promise before terminating. Verify that source, then run held-disposal SIGTERM proof with the workaround bypassed, the real bridge lifecycle suites, and the exact-head repeated receipts. If upstream awaits a different disposal entrypoint, move the observation gate to that actual promise while retaining explicit release and both process-exit assertions. A changed listener name or a locally green ungated run alone is not removal evidence.
