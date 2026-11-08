# B3 central: machine login decisions — trace

This slice holds the parts of B3 machine login that don't depend on storage, built in
`collaboration-central-core` with no runtime. The hosts inject the clock, ID source and
enrollment store. SQL, migrations, D1/SQLite adapters, the enrollment's stamp pin,
`SetTokenLifetime`, `machine_environment` values and anything Router-side are out of scope.

## Contract alignment with the issuer mirror

Central's Rust contract matches the issuer's `machine-login-contracts.ts` (switchboard
`item2/b3a-device-grant` at `4d7e0e1e`) field for field, camelCase on the wire.

| Shape | Change |
| --- | --- |
| `DecideDeviceLogin` | adds `permissionChecked: "machine:enroll"`. `label` is trimmed, 1–120 UTF-16 code units. `allowedWorkspaces` holds at most 100. `approverSecondFactorAt` is a required key that may be null. Adds optional `formerServiceId` (UUIDv4 or UUIDv7). |
| `DeviceLoginRefusal` | `notOwner` is now `notPermitted`. Adds `formerServiceLinkedElsewhere {}`. `withinSeconds` is positive. |
| `MachineTokenDecision.grant` | adds optional `superseded: EndedMachineClient { enrollmentId, clientId }` |

Proof: the transcribed literals in `issuer_decision_contracts.rs` round-trip exactly. The
mirror's refusals are in `machine_login_mirror_negatives`, and serde and the generated
schema each refuse every one of them. Three seeded reds each failed at their own assertion:
a missing `approverSecondFactorAt` accepted, the UTF-16 bound removed, and the
former-service version check removed.

Central is stricter than the mirror in three places. The issuer's outputs never reach them:

- A label is also refused when it holds C0 controls other than tab and newline, or runs
  over 256 UTF-8 bytes (the board's existing `MachineLabel` rule).
- UUIDs must be canonical lowercase; `z.uuid()` accepts uppercase.
- An explicit `null` for `formerServiceId` or `superseded` is read as absent. That follows
  central's convention for optional keys; central never writes null.

## Assumptions for the owner page

- **`formerServiceLinkedElsewhere {}`** is a new bounded device-login refusal. It answers a
  re-key whose former service id is already linked to a different installation or
  organization. It reveals nothing beyond "not linkable here": no organization, enrollment
  or installation. The variant was sent to the switchboard B3a Implementer.

## Stand-ins

Each entry: the seam, what stands in, the assumptions it encodes, what closes it.

(Filled as each one lands.)
