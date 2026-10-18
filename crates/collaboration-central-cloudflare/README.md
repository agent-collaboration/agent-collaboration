# Central Worker shell

Run commands from the repository root. This standalone wasm workspace uses
workers-rs and worker-build 0.8.7. Local runtime tests use Miniflare
5.20261012.0-beta (workerd 1.20261012.1), pinned. It is excluded from
the native Cargo workspace. Its four Durable Object classes use the same
synchronous central core as the native shell, inside the supported
`Storage::transaction()` callback. Effects run after commit. Integer bindings
and result storage classes are declared by the core; the shell never rounds
integers through JavaScript numbers.

Build a deployment candidate without test features:

```sh
worker-build crates/collaboration-central-cloudflare --no-opt -- --locked --no-default-features
python3 scripts/tooling/check-central-worker-bundle.py production
```

The checked bundle is `build/index.js` plus `build/index_bg.wasm`. No deployment
is part of this command. Deploys use the cf CLI's config format: the public
`cloudflare.config.ts` template disables workers.dev and preview URLs, declares
the four SQLite Durable Object classes and their bindings, and contains
placeholders for the Worker name, team domain, session/admin app AUDs and owner
identity. `wrangler.config.ts` sets `noBundle`, so `cf deploy`'s build step
uploads the guarded `build/` files byte-for-byte rather than rebundling them;
the bundle guard checks both templates. The account, dedicated hostname
(`domains`), real values and the pinned `cf` and Wrangler versions belong in
the private ops repo. Admin uses a signed owner assertion, with no stored
secret.

The native lifecycle harness owns a real workerd process through Miniflare,
pinned, which it drives directly. Its local-only outbound service redirects the
synthetic issuer's exact certs URL to an owned HTTP JWKS fixture, and rejects
other external fetches. The production Worker code, configuration-derived
certs URL, WebCrypto and Cache stay unchanged. There is no key-source override
or Cargo test feature for Access verification. The harness waits for Ready
and stops/reaps workerd and the fixture after the test. Supply the installed Node
executable and the pinned Miniflare entrypoint (`dist/src/index.js`), an isolated persistence
directory, and a free loopback port:

```sh
cargo run --locked -p collaboration-central-conformance --example workerd_lifecycle -- default node /path/to/miniflare/dist/src/index.js /absolute/test/persistence 8957
```

This exercises all Access rejection cases and real HTTP/WebSockets with
upgrade authentication, and verifies that the default build has no test controls. It uses synthetic credentials
only. Use a different isolated persistence directory for the fault build:

```sh
worker-build crates/collaboration-central-cloudflare --dev --no-opt -- --locked --features conformance-faults
python3 scripts/tooling/check-central-worker-bundle.py faults
cargo run --locked -p collaboration-central-conformance --example workerd_lifecycle -- conformance node /path/to/miniflare/dist/src/index.js /absolute/fault-test/persistence 8957
```

The fault feature implies `conformance` and is off by default. It compiles one
G18 panic injection after a completed SQL write, with a function-local panic
lint allowance explicitly authorized for this observation. The harness bounds
the request, requires the platform event confirming the injection ran, restarts
the entire owned workerd, and records the persisted rows, fence and prefix
without guessing the outcome. CI finishes by rebuilding and checking the
deployment candidate with no test features.

The shared suite covers forwarding, Access, copy, exact numeric transport,
session and transfer rules. Forwarding reserves one of 16 sender slots before
routing and retains the original 30-second deadline. Correlation rows contain
IDs and deadlines; post and reply envelopes stay transient. The origin label
comes from machine enrollment. The holder's board admission is a fixed M4
stand-in in these tests.

Run the separate two-native-client demonstration against the fault build:

```sh
cargo run --locked -p collaboration-central-conformance --example workerd_lifecycle -- forward-app node /path/to/miniflare/dist/src/index.js /absolute/forward-demo/persistence 8957
```

It verifies a forwarded post round trip, then holds the sixteenth route lookup
while another forward races for the last slot. The second gets `Overloaded`;
after releasing the lookup, all 16 reservations answer and a fresh post succeeds.
Only the controlled route hold uses the conformance endpoint; posts and answers
use real authenticated WebSockets.

The import/claim demonstration also uses an owner admin client and two native
machine sessions, against the fault build:

```sh
cargo run --locked -p collaboration-central-conformance --example workerd_lifecycle -- import-app node /path/to/miniflare/dist/src/index.js /absolute/import-demo/persistence 8957
```

It imports a project, reads its unheld copy, holds A's fence while B's overlapping
claim gets `AlreadyHeld`, grants A epoch 2, applies its first piece at the imported
head, then delivers B's forwarded post to A and returns the fixed M4 holder reply.
The shared suite additionally checks staging, import identity, rollback,
re-imports, schema support and a registry restored behind a claimed copy.
Physical restart checks the interrupted claim and the copy's persisted directory
outbox: alarm retry reaches a test-only Fleet receiver and stops after its
acknowledgement. That receiver records update messages only. Real directory
entries and receiver integration, plus `/admin/v1/import/fleet`, wait for M3.5.

Physical process restart additionally verifies forwarding deadline recovery
after loss of its transient carry, rollback, simultaneous
complete-prefix reads, numeric storage classes, interrupted fencing and lost
answers. The Evict control separately drops the Rust runtime handle cache; it
does not claim to force physical Durable Object eviction. Hibernated sockets
use supported accept/attachment callbacks. Local workerd evidence does not
establish deployed durability, TLS transport, or cross-machine behavior.

Fleet admission is still the named M3.5 boundary: fixed holder fixtures supply
FleetAcceptance until the real M2 admission crate is integrated. Capacity,
clock, answer loss and interruption controls are test-only. Injected capacity
failure does not prove physical Cloudflare quota exhaustion.

The edge verifies RS256 with WebCrypto SubtleCrypto against a cached JWKS from
`https://<configured-team>/cdn-cgi/access/certs`, selecting `kid` from `keys`
and refreshing once when unknown. Cache lifetime is at most one hour. The
core checks issuer, the app that owns the path, expiry/not-before, caller class
and owner identity. Registry authentication also matches the signed Access
client ID and machine credential to one enrollment. Revocation closes its
existing logical session and refuses a new upgrade.

Local tests sign assertions with the deliberately public test RSA fixture.
They do not prove actual Google sign-in, service-token policies, header stripping
or the dedicated hostname's deployed network gate. Those await authorized
staging proof. The prior throwaway experiment is design evidence for transport;
the local suite proves the new native RSA and Worker WebCrypto verification.
