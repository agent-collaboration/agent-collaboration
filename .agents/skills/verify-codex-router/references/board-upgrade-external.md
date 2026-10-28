# Import into an external central (OB5 / G4)

## Current entry

The external wrapper retired in 0.1.102. Local publication/replay/restart coverage now lives in `crates/board-legacy-upgrade/tests/upgrade_binary_e2e.rs`; use `cargo test --locked -p board-legacy-upgrade --test upgrade_binary_e2e -- --test-threads=1` after CI-equivalent artifact preparation. `verification/import-run.ts` is the separately commissioned sealed-copy Mac/local-Tokio entry. The current two-machine/Worker entry is `node verification/run.ts "$PRIVATE_DECLARATION"`, governed by `verification/declaration.ts` and `verification/case-registry.ts`. These entries do not establish the entire former external publication matrix; an obligation without a retained mapped case remains unavailable.

## Retained obligations and historical evidence

The sections below preserve the governing obligations and earlier qualification limits. Descriptions of retired wrappers and their inputs are historical; they are not current command contracts. Use the retained source above to determine actual coverage, and report any obligation without a current route as unavailable. This documentation change supplies no new runtime proof or operating authority.

The historical external wrapper used the actual signed
upgrade CLI, machine WebSocket session and isolated model-free Host. It does not
deploy, enroll an installation, start or stop the external central, or obtain
real legacy stores. Use only synthetic legacy copies. A staging run requires a
separate commission naming the exact HTTPS origin and conformance controls.

The expected outcome is board Verification Design OB5 and §5e, and spec 3
FL-15–FL-17. The existing legacy derivation and native snapshot checker remain
the oracle. This extension adds transport and observation, not another import
model. The local qualification uses synthetic signed JWKS; it cannot prove real
owner sign-in, Access policy or header transit, TLS, Worker deployment, Durable
Object eviction or production data.

## Inputs and authentication

G4's caller supplies:

- The final source commit and a receipt for five frozen signed artifacts with SHA256:
  `target/debug/codex-router-board-upgrade`,
  `target/debug/board-upgrade-native-observer`,
  `target/debug/board-upgrade-central` (local qualification only),
  `target/debug/examples/board-debug-runtime`, and
  `target/debug/agent-collaboration`. Their owner builds and signs them normally,
  then copies them into one private, flat artifact directory. Supply that set's
  receipt; the driver verifies and executes the copies without rebuilding or
  re-signing. Keep the helper dependency pin equal to the product's SDK pin.
- An explicitly authorized central **origin**, without a path, user info, query
  or fragment. Staging must use HTTPS. The same origin serves `/v1/session`,
  normal admin import, owner Copy Health/status and the owner-only
  `/admin/v1/conformance/runtime` control. Ports 8787, 8788 and 43127 refuse.
  A production Worker without conformance controls cannot run the fault case.
- One disposable, already-enrolled staging machine's installation, UUIDv7
  peer, authoritative receiver and relay identities, plus its machine credential
  and Access service-token client ID/secret. Supply **environment variable
  names**, not their values, in the plan. Take identities from actual enrollment
  and deployed configuration; never derive a PeerId from a ServiceId.
- A canonical owner-private system-temp runtime parent and an absent output
  directory whose existing parent is also private system-temp state. The driver
  creates its isolated Host and credential files there and deletes them after
  its children settle. Evidence is separate from that private runtime.
  Use a short `/private/tmp` parent: the resulting Host `control.sock` path must
  be shorter than 104 encoded bytes. Longer paths refuse before Host startup.

The owner signs in to that origin using the documented product path:

```sh
cloudflared access login --quiet "$OB5_CENTRAL_ORIGIN"
```

`--quiet` avoids printing the JWT. The driver reads the existing app token with
`cloudflared access token -app=<origin>`, matching
`collaboration-client/src/owner_access.rs`, and holds it only in memory. It
supplies `cf-access-token` to Access; Access supplies the signed assertion the
central verifies. The loopback CLI proxy removes downstream authentication
headers and forwards only to that one origin. Its local debug assertion is a
nonsecret transport sentinel, never an owner assertion. Machine connections
present the declared service-token headers plus the bearer machine credential.
Redirects are refused, including on WebSocket admission.

Use an existing `op run` wrapper when credentials are vault-managed. Do not
print tokens, resolved secrets or a rendered environment. Credential failures
stop the drive. Do not log in, enroll, deploy or change Access as a workaround.

## Plan and entry

`ExternalPlan` in `scripts/verification/board_upgrade_external_plan` (retired wrapper) owns the
validated format. `artifact_receipt` is the single source for artifact paths,
hashes, signatures and the build pin. The
checkout may advance through apparatus-only commits; any committed or dirty
product difference from `expected_head` refuses before each CLI action.

```json
{
  "repository": "/absolute/declared/checkout",
  "expected_head": "<40-character-final-build-commit>",
  "artifact_receipt": "/absolute/private/build-set/receipt.json",
  "endpoint": "https://<commissioned-conformance-central-host>",
  "authentication": "ownerAccess",
  "runtime_parent": "/private/tmp/<private-task-parent>",
  "output_directory": "/private/tmp/<private-evidence-parent>/ob5",
  "machine_environment": {
    "installation": "STAGING_A_INSTALLATION",
    "peer": "STAGING_A_PEER",
    "receiver": "STAGING_AUTHORITATIVE_RECEIVER",
    "relay": "STAGING_RELAY_PEER",
    "credential": "STAGING_A_MACHINE_CREDENTIAL",
    "access_client_id": "STAGING_A_ACCESS_CLIENT_ID",
    "access_client_secret": "STAGING_A_ACCESS_CLIENT_SECRET"
  },
  "timeout_seconds": 300
}
```

These are input placeholders, not an executable staging declaration. A plan
contains no token, secret reference or credential path. `localFixture` and its
assertion/machine-edge paths are exclusively the local qualification route;
`ownerAccess` refuses those overrides.

The retired command is unavailable; use the current entry and declaration above.

`websockets` is the optional G4 wire transport; it is loaded only during a
machine drive. The existing entry/help/local-publication commands do not need
that package. A preconnected socket prevents redirects; every answer must name
the actual outstanding frame ID, and snapshot/feed continuations must advance.

## Cases and deciding observations

The driver clones the retained complete, empty-project/role-history and mixed
watch fixtures into fresh namespaces. It remaps all references together and
checks foreign keys. No source fixture or owner store changes. Every case keeps
the dry-run report, actual CLI exits and an independent native preparation
observation. The real central's snapshot pages must preserve every prepared
table and row, after the native observation has passed the independent legacy
oracle, including all reader/mode bookmark and watch coverage.

For each positive import, require actual admin publication; owner workflow
Complete, Copy Following/Unheld at epoch one with its exact acknowledged
manifest; unknown imported time in the owner summary and real fleet feed and
snapshot; then relabelled same-input `alreadyImported`, identical retained
import identity/header/phase and stable fleet digest. Changed content under
those same IDs must return the actual typed `importConflict`. A separate
workspace/board collision pair must refuse before any HTTP request and keep
both conflicting IDs.

The **before-publication** case holds the real first Complete request after
pages were staged and reaps only the CLI. Owner progress must remain Reserved;
the rerun must finish the original attempt.

The **after-publication/before-completion** case arms the existing Copy runtime
`dropNextObjectReply` targeting Fleet immediately before Complete. It retains
the actual dropped envelope and owner progress, requires the published Copy
and unfinished workflow, then reruns the CLI under the same semantic identity.
This is distinct from the older local wire case that loses an already-completed
Imported response. If the actual fault misses the interior checkpoint, report
that as a driver gap; do not count the completed-response case in its place.

The isolated Host first receives the imported project through its public
directory, then claims it, installs its acknowledged
prefix before use, gets epoch two, performs a guarded first accepted write and
waits for its exact central acknowledgement. Read its public feed and work
view, its stopped project file/cache and owning reopen. A later import replay
must preserve the live holder, fence and acknowledged prefix.

## Local qualification and evidence

Use the existing real Tokio central, signed fixture Access assertions, normal
owner enrollment for one disposable synthetic machine and the same external
driver. This creates and reaps only its local test central/edge/Host. It neither
signs in to Access nor reads the real credential environment:

The retired command is unavailable; use the current entry and declaration above.

The artifact input is the full frozen-build receipt object: `pin`,
`artifactDirectory` and `artifacts`. Each artifact entry has `path`, `sourcePath`,
`sha256`, `sourcePin`, `identifier`, `team` and `strictSignatureVerified`.
`sourcePath` identifies its normal build location above; `path` identifies the
copy in the owner-private artifact directory. The receipt must cover all five
artifacts above and bind each to the declared pin. Actual hashes and strict
debug signatures are checked before execution. No build or signing occurs.
Fixture-authenticated local evidence remains separate from the later staging
result. Staging must show its actual deployed source/artifact binding, real
Access-issued assertions and header transit, machine enrollment binding and
deployed durability. This route alone does not prove OB7 deploy/eviction or
OB4 cross-machine behavior.

Retain `external-journey.json`, per-case reports, snapshot pages, fleet reads,
owner status, exact fault checkpoint, Host receipt and cleanup. Reopen evidence
after teardown. The external central stays running and synthetic imported IDs
stay there for G4's controller; this driver never retires or deletes them.
Private local machine files are deleted only after the owning Host stops.

Apparatus checks, separate from product assertions:

The retired command is unavailable; use the current entry and declaration above.
