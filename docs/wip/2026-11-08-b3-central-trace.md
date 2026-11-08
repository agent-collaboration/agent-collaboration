# B3 central: machine login decisions — trace

This slice holds the parts of B3 machine login that don't depend on storage, built in
`collaboration-central-core` with no runtime. The hosts inject the clock, ID source and
enrollment store. SQL, migrations, D1/SQLite adapters, the enrollment's stamp pin,
`SetTokenLifetime`, `machine_environment` values and anything Router-side are out of scope.

## Contract alignment with the issuer mirror

Central's contract matches the issuer's `machine-login-contracts.ts` (switchboard
`item2/b3a-device-grant`) in shape, camelCase on the wire. Where validation differed, the
Lead ruled each row against the Program Design; where the PD is silent, the owner of the
shared type decides.

| Shape | Central |
| --- | --- |
| `DecideDeviceLogin` | adds `permissionChecked: "machine:enroll"`. `label` is the board's `MachineLabel` (PD:880, PD:576): trimmed, 1–256 UTF-8 bytes, no C0 controls except tab and newline. `installation` must be a UUIDv7 here only (B3:126); `InstallationId` stays shared because older UUIDv4 rows still appear in lookups. `allowedWorkspaces` holds at most 100. `approverSecondFactorAt` follows PD:571 (omitted or null reads as none; never written as null). Adds optional `formerServiceId`: UUIDv4 or UUIDv7, RFC variant, canonical lowercase. |
| `DeviceLoginRefusal` | `notOwner` is now `notPermitted`. Adds `formerServiceLinkedElsewhere {}`. `withinSeconds` is positive. |
| `MachineTokenDecision.grant` | adds optional `superseded: EndedMachineClient { enrollmentId, clientId }` |

The mirror changed on its side to match: the label byte check, C0 refusal, and lowercase
UUIDs, including `formerServiceId`. `machine_login_mirror_vectors` reads a byte-exact copy of
the mirror's 95 validation vectors (switchboard `86d79179`, in
`contracts/issuer-mirror/`). Central refuses exactly the 59 the mirror refuses and accepts
the 36 it accepts, writing each byte for byte. The one named exception is row 6's null
versus an omitted `approverSecondFactorAt`. With the label seeded as an unbounded string,
36 of the 95 vectors disagree.

Proof:

- The literals in `issuer_decision_contracts.rs` round-trip exactly.
- The bounds' negatives are in `machine_login_mirror_negatives`. Serde and the generated
  schema each refuse every one.
- Each of these seeded reds failed at its own assertion:
  - a missing `approverSecondFactorAt` refused;
  - the label as an unbounded string;
  - the UUIDv7 installation check removed;
  - the former-service version check removed.

## Assumptions for the owner page

- **`formerServiceLinkedElsewhere {}`** is a new bounded device-login refusal. It answers a
  re-key whose former service id is already linked to a different installation or
  organization. It says nothing beyond "not linkable here": no organization, enrollment
  or installation. The switchboard B3a Implementer has the variant.
- **`allowedWorkspaces` holds at most 100 workspaces.** The PD (PD:881) states no bound.
  The issuer and central both enforce 100.

## Stand-ins

Each entry gives the seam, what stands in, the assumptions it encodes and what closes it.
Each entry is added as its stand-in lands.
