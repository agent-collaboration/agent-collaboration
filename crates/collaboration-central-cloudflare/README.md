# Central Worker shell

Run these commands from the repository root. The Worker is a root Cargo
workspace member. It uses workers-rs and worker-build 0.8.7; local runtime
proof uses Miniflare 5.20261012.0-beta and workerd 1.20261012.1.

Use the Node and pnpm versions pinned in `verification/package.json`. Install
tooling dependencies with `pnpm --dir verification install --frozen-lockfile` before
running the bundle checks and local conformance commands below.

Build and check a deployment candidate without test features:

```sh
worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt -- --locked --no-default-features
node verification/tooling/check-central-worker-bundle.ts production
```

The root `worker-release` profile preserves the former standalone Worker's
`lto = true`, `opt-level = "s"` and `codegen-units = 1`. Native release builds
continue to use Cargo's separate `release` profile.

The checked bundle is `build/index.js` plus `build/index_bg.wasm`. These
commands do not deploy it. The existing deployment flow uploads that prebuilt
artifact: `wrangler.config.ts` retains `noBundle`, and the public cf-format
`cloudflare.config.ts` disables workers.dev and preview URLs. Account, routing
and real Access values belong to the private ops configuration. The public
template contains placeholders and five SQLite Durable Object bindings.

Fill `CENTRAL_DELIVERY_CONFIG` with JSON containing the deployment's immutable
UUIDv7 relay identity, for example `{"relayPeer":"<deployment-relay-uuidv7>"}`.
This identity is independent of `CENTRAL_REGISTRY_CONFIG`'s authoritative Copy
receiver. Missing, malformed or non-UUIDv7 values refuse initialization; there
is no default relay identity. Keep the real deployment values in private ops.
The public binding-presence check runs from the root with
`node --test crates/collaboration-central-cloudflare/tests/public-template.test.mjs`.

The Worker adapts the shared asynchronous core to Durable Object storage and
transport. The legacy synchronous central shell and its conformance-only
execution path have been removed. Admin requests require a signed owner
assertion; the shell does not store an admin secret.

For local conformance, build the dev artifact with the current test controls:

```sh
worker-build crates/collaboration-central-cloudflare --dev --no-opt -- --locked --features conformance-faults
node verification/tooling/check-central-worker-bundle.ts faults
cargo run --locked -p collaboration-central-tokio --features conformance --example composed_scope_workerd -- runtime-controls node /path/to/miniflare/dist/src/index.js /absolute/fresh/persistence 8957
```

The same process wrapper owns readiness, shutdown and physical restart for the
retained examples. Supply a fresh persistence directory for each scenario.
`composed_scope_workerd` accepts its mode first: `composed-copy-suite`,
`composed-transactions`, `composed-numeric`, `runtime-controls` or
`composed-fleet-conflict`, followed by Node, Miniflare, persistence and port.
`step_scope_workerd` and `bootstrap_scope_workerd` accept those four arguments
without a mode. Their fixtures use owner-verified
`/admin/v1/conformance/fleet-receipt` and `/admin/v1/conformance/bootstrap`.

Normal session cases use a mode after the four runtime arguments, for example:

```sh
cargo run --locked -p collaboration-central-tokio --features conformance --example session_scope_workerd -- node /path/to/miniflare/dist/src/index.js /absolute/fresh/session-persistence 8957 access-sessions
```

The harness redirects only the synthetic issuer's certs URL to its owned local
JWKS fixture. Worker WebCrypto verification remains real. No local run proves
Google sign-in, deployed Access policies, TLS or cross-machine behavior.

Retired launchers and their historical oracles were recorded in the 0.1.102
process logs, which are kept outside the public tree.
Complete normal Worker session/holder equivalence and G18 panic rollback with
subsequent store usability remain PR3 obligations. The removed legacy fixtures
do not count as proof of those behaviors on the shared core.
