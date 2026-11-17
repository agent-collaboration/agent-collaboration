# Central Worker shell

Run these commands from the repository root. The Worker is a root Cargo
workspace member. It uses workers-rs and worker-build 0.8.7; local runtime
proof uses Miniflare 5.20261012.0-beta and workerd 1.20261012.1.

Use the Node and pnpm versions pinned in `verification/package.json`. Install
tooling dependencies with `pnpm --dir verification install --frozen-lockfile` before
running the bundle checks below.

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
and real sign-in values belong to the private ops configuration. The public
template contains placeholders and binds `CENTRAL` to each organization's central
and `WORKSPACE_STORE` to one store per workspace named by its workspace id. A
workspace store holds tables and runs the statement batches central sends it; it
has no rules of its own. Deploying the new `WORKSPACE_STORE` class needs a
deployment-side Durable Object migration entry; that belongs to the deployment
deliverable (E), and nothing in this repository deploys it yet. The template no
longer exports the old `ProjectCopyObject` and `FleetObject` classes: deploying
it drops those objects and their stored data, a separate one-way deploy step.
The public binding-presence check runs from the root with
`node --test crates/collaboration-central-cloudflare/tests/public-template.test.mjs`.

The Worker adapts the shared asynchronous core to Durable Object storage and
transport. The public Worker has no fetch entry: it only hosts the
`CentralObject` and `WorkspaceStoreObject` classes. The old public Access
edge and its conformance path have been removed. The `test-support` bundle
alone mounts a fetch entry, for its storage and workspace case routes.

## Private organization sign-in link

The `link` feature builds a separate Worker for the five private sign-in routes.
Its `CENTRAL` binding addresses the public Worker's `CentralObject` namespace;
it owns no separate Durable Object namespace. The deployment template is
`cloudflare-link.config.ts`, with public URLs and preview URLs disabled.

Build and inspect its production artifacts from the repository root:

```sh
worker-build crates/collaboration-central-cloudflare --profile worker-release --no-opt --out-dir build-link -- --locked --no-default-features --features link
node verification/tooling/check-central-worker-bundle.ts production --bundle-directory crates/collaboration-central-cloudflare/build-link
```

Trust settings belong to the public central object: `CENTRAL_SIGN_IN_ISSUER`,
`CENTRAL_SIGN_IN_AUDIENCE`, `CENTRAL_SIGN_IN_JWKS_URL`, `CENTRAL_ADMIN_AUDIENCE`,
`CENTRAL_PUBLIC_ORIGIN`, and `CENTRAL_SIGN_IN_SERVICE_ID` for person admin calls.
The runtime verifies organization stamps and person proofs with its injected clock,
then applies static guarded batches under the organization turn. The active routes
cover provisioning, statement digest, revision delivery, change records and unknown
workspace lookup. Workspace creation, existing-workspace view mapping and machine
login decisions await their separate approved contracts.
