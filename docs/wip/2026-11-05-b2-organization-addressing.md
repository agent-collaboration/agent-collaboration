# B2 organization addressing

## Delivery hold (2026-11-06)

Copy routing at `0337d264907f6a987612f3eb314086d346199afa` is **unaccepted and
unshipped**. The full Tokio run exposed 48 existing D1 Copy failures: valid
local Health/Repair/Migrate and typed uninitialized operations are gated by an
empty reference map; imported seed arrangers and native/process reopens lose
that map while the Copy data and follow-up journal remain. An ephemeral routing
stand-in cannot be a mandatory admission policy for those existing paths.

The Lead chose **B1 first** (the owner decision about central's proposed tables,
including the private Copy reference). No legacy-only namespace assignment,
fallback organization, revert, reset, or history rewrite is authorized. Copy
routing code stays frozen until that decision; the scoped Fleet resolver/naming
work remains available for its independent proof.

The 49th Tokio failure was the separate Fleet SQL observer in a combined test
process. Its unchanged exact case passed under nextest (one selected, one
passed, exit 0). Preserve the SQL oracle and run integration tests under nextest
with one process per test. This does not close the 48 Copy regressions.

The architecture check reports a further central-core baseline shrink with no
new edges. On resumption, perform a shrink-only `ARCHITECTURE_BLESS=1` refresh,
inspect the before/after JSON, and rerun the ordinary gate. Do not bless growth.

The physical Fleet address now includes its validated organization. Registry,
Fleet, Session and Delivery callers carry that organization; logical Copy
callers carry it only in memory. A physical Copy address remains its database
ID. Shared envelopes require the organization and reject a different addressed
Fleet before dispatch. There is no global-address reader or conversion code.

Each deployment must explicitly configure its legacy organization:

- Native `TokioSettings.legacy_organization`; the central CLI requires
  `--legacy-organization-id` when serving.
- Cloudflare `CENTRAL_LEGACY_ORGANIZATION_ID`.

Only that organization uses `fleet.db` / DO name `fleet`. Other organizations
use `fleet-<organization>.db` / their organization ID as the DO name. Native
reopening rejects the noncanonical `fleet-<legacy-organization>` alias.
Legacy Access entrypoints bind only to the configured organization. New B2 token
routes remain a separate release boundary.

## Copy reference stand-in

`CopyFleetReferencePort` resolves the host-private opaque reference into the
logical Fleet route. Its current in-memory adapter is intentionally not durable.
Only a validated creation/seed admission for an actually uninitialized Copy can
establish a missing reference. A different organization cannot replace an
existing binding. After process restart, an existing Copy with a missing
reference refuses calls rather than inferring its organization from replicated
data or a caller. The SQL-gated native and CF adapters must restore that
reference before ordinary Copy restart journeys can pass.

The wired stand-in is not an acceptable delivery point: the failures above
violate existing D1 behavior. Its focused tests prove only the tested component
properties, not persisted Copy routing, recovery, organization provisioning or
real token authorization.

## Deployment order

B2 must land before the D1 production rollout. Fresh storage at B2 staging
deployment remains a dependency: the owner must explicitly authorize the reset,
and production-state evidence must be checked before relying on a reset/first-Fleet
assumption. This work authorizes no reset. There is no Cloudflare legacy-store
preservation claim. CF restart proof starts with new-shape storage.

The signed predecessor claim and private-read response shape remain pending
owner decisions. Their existing pure CAS/verifier code is a component candidate,
not an adopted wire contract.

## Product proof surfaces

Native library tests use actual object files, SQL stores and a fixed clock/ID
source to check three-Fleet isolation, same-file restart, the reserved alias,
and refusal to rebind an existing Copy after losing the stand-in.

`organization_location_workerd` uses the real compiled Worker, actual Durable
Objects and owner verification against a local JWKS. Its owned process uses
readiness/output and exit events. An explicit conformance-only fixed clock and
ID source replace wall time; production clock behavior is unchanged. Three
independent persisted DO metadata markers must survive process restart through
the new address shape. This is physical routing and metadata continuity proof,
not SQL-gated organization provisioning or Fleet domain-transaction proof.

At exact `0337d264907f6a987612f3eb314086d346199afa`, the Sunclaw CF run
passed: Worker build, bundle guard and example each exited 0. Three independent
Fleet metadata markers survived the real owned process restart; both processes
were reaped. The raw logs and artifact hashes were directly inspected. This
closes that bounded physical Fleet proof and leaves the Copy hold unchanged.

The CLI cases live in the library, not the binary target. The exact filter
`cargo test --locked -p codex-router-cli --lib central_command::arguments::tests -- --nocapture`
ran both required cases at the same clean head: 2 passed, 0 failed, 312 filtered
out, exit 0. The earlier binary filter selected no tests and provides no proof.
