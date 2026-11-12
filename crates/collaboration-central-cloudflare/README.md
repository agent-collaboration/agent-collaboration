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
template contains placeholders and one SQLite Durable Object binding, `FLEET`.

Fill `CENTRAL_DELIVERY_CONFIG` with JSON containing the deployment's immutable
UUIDv7 relay identity, for example `{"relayPeer":"<deployment-relay-uuidv7>"}`.
Enrollment answers return it together with `CENTRAL_REGISTRY_CONFIG`'s
authoritative receiver. Missing, malformed or non-UUIDv7 values refuse initialization; there
is no default relay identity. Keep the real deployment values in private ops.
The public binding-presence check runs from the root with
`node --test crates/collaboration-central-cloudflare/tests/public-template.test.mjs`.

The Worker adapts the shared asynchronous core to Durable Object storage and
transport. The legacy synchronous central shell and its conformance-only
execution path have been removed. Admin requests require a signed owner
assertion; the shell does not store an admin secret.

The Worker keeps only machine enrollment, Access verification and session
admission/upgrade on the Fleet object. Its former Copy object, mail relay and
local workerd conformance harness were removed with the old board stack; the
workspace storage redesign brings their replacements and new proof.
