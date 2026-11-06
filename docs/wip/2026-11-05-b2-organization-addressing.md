# B2 organization addressing

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

This stand-in permits scoped development now. It does not prove persisted Copy
routing, recovery, organization provisioning or real token authorization.

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
