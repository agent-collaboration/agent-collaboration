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

## The decisions

`organization_authority/machine_login/` holds them. It sits outside the module cycle and
depends on `identity_contract`, `person_request` (B2's JWT parsing, header and audience
types) and the issuer key cache and verifier. Each decision is a pure function over one
transaction's `EnrollmentFacts`. It returns its answer plus a closed list of
`EnrollmentChange`s, which the port applies all or nothing. The clock is read inside the
transaction, and IDs come from the injected source; a non-UUIDv7 ID is refused.

| Public entry | Rules |
| --- | --- |
| `decide_device_login` | SD8 (within 5 s of receipt, on the clock read in the transaction); idempotent on the request ID, where a different body is `notPermitted`; a machine claimed by another organization; unknown workspaces; the second factor within 900 s unless the organization never had an enrollment; base generation; allowances and `allowanceChanged`; re-key once |
| `decide_machine_token` | BA1 fence at the first Grant; supersession (`machineRevoked`, reason superseded) and `machineEnrolled` in that transaction; repeat Grants read live and only move `last_grant_at`; client = enrollment ID at the first Grant, the recorded client after; idle expiry at 30 days; approval lapse at the device-code lifetime; `grant_expires_at = now + lifetime` |
| `admit_machine_connection` | the token, enrollment and presented installation and peer name one machine; organization; superseded; not active; client; idle expiry on read |
| `revoke_machine_enrollment` | revoked (owner), generation bumped, allowances withdrawn, `machineRevoked` + `allowanceChanged`; answers `Applied { ended }` and a close instruction; `notFound` with nothing written |
| `MachineTokenVerifier::verify`, `admit_machine_token` | C9 in order: EdDSA header and kid; pinned issuer (before any key fetch); signature; audience; the clock read after the fetch: `now < exp + 60`, `iat ≤ now + 60`, `now < grant_expires_at`; typed machine claims (other registered claims unread) |
| `renew_machine_token` | the fresh token through C9, naming the connection's own installation and enrollment, then admission; `MachineTokenRenewal` (`fleet-identity-protocol`) answers it |

**Token bounds through the wait (Advisor point 01a1bc73-34bc, Lead ruling 01a1bc77).**

- **The defect at 5826592fb:** the check ran before the port wait, and admission sampled its clock only for idle expiry. So a token verified 1 ms before its grant ended could be admitted or renewed after the wait.
- **The fix:** `VerifiedMachineToken` carries `exp` and `grant_expires_at`. Admission and renewal recheck `now < exp + 60` and `now < grant_expires_at` on the clock read inside the admitting transaction. `AdmittedMachine.token_expires_at` is the earlier of `exp + 60` and the grant, as the host's timer value.
- **Proof:** for both entries and both bounds, held through `before_next_decision`: 1 ms before the bound is admitted or renewed; equality and 1 ms after are refused as token expiry, with the store unchanged. The timer is tested with each bound ending first and with a tie.
- **Seeded reds:**
  - with only the in-transaction recheck bypassed, the equality case is admitted or renewed;
  - with a raw-exp timer, or one without the grant, the timer test fails.

## Assumptions for the owner page

Each of these was added by an agent or ruled by the Lead; the PD doesn't state them.

- **`formerServiceLinkedElsewhere {}`** is a new bounded device-login refusal. It answers a
  re-key whose former service id is already linked to a different installation, or that
  would merge into an installation that already has rows here. It says nothing beyond
  "not linkable here": no organization, enrollment or installation. The switchboard B3a
  Implementer has the variant.
- **`allowedWorkspaces` holds at most 100 workspaces.** The PD (PD:881) states no bound.
  The issuer and central both enforce 100.
- **A repeat request ID with a different body** is refused `notPermitted`, with nothing
  written.
- **SD8** is measured from the host's receipt to the clock read in the applying
  transaction. Exactly 5 s is within.
- **"The organization's first machine"** (PD:882) means the organization has no
  enrollment in any state, ever.
- **Idle expiry withdraws the machine's allowances.** It doesn't move the generation and
  writes no record.
  - Owner question: idle expiry has no change kind of its own.
- **Records:**
  - `machineEnrolled` is written at the first Grant, never at approval.
  - Supersession writes `machineRevoked` (reason superseded) in the same transaction.
  - Records from approvals and first Grants carry the approver as origin, with
    `machine:enroll`. Revocation's carry the revoker, with `machine:revoke`.
- **A former service id that names no installation here** links nothing.
- **An approval lapses** once `now ≥ approved_at + device-code lifetime`. `approved_at` is
  central's own clock in the approving transaction; the lifetime is one setting,
  10 minutes (PD:1292).
- **The renewal answer's mapping:**
  - a fresh token for another installation → `wrongInstallation`;
  - one for another enrollment of the same machine → `enrollmentNotActive`;
  - admission's `unknownMachine` → `wrongInstallation`;
  - superseded, another organization or inactive → `enrollmentNotActive`;
  - a token refused for time → `tokenExpired`;
  - any other check refusal → `tokenInvalid`.

## Stand-ins

### The Fleet store behind `MachineEnrollmentPort`

- **Seam:** `MachineEnrollmentPort::transact(query, decide)`: one transaction reads the
  facts a query names and applies the decision's changes, in order, all or none.
- **Stand-in:** `InMemoryEnrollments` (test-only). It applies the changes to a copy. It
  checks the port contract against the state before: at most one enrolled enrollment per
  installation, which is the installation's active one, and a generation that moves by
  one, only beside a first-Grant activation or an owner revocation. It swaps the copy in
  only if those hold.
- **Assumptions it encodes:**
  - Workspaces, other organizations' claims and former-id links are readable in the same
    transaction.
  - Re-keying moves the installation row, its enrollments and its allowances.
  - Sessions and board relationships aren't modelled.
- **What closes it:** the reviewed SQL for `machine_enrollments` (reshaped),
  `machine_allowances`, the generation and former-id home, and the D1/SQLite adapters
  passing these same decision tests.

### The grant lifetime

- **Seam:** `MachineLoginSettings::grant_lifetime`.
- **Stand-in:** a value the host supplies, until `SetTokenLifetime` has a home.
- **What closes it:** the organization's stored lifetime.

### The close instruction

- **Seam:** `MachineRevocation::close`.
- **Stand-in:** a returned value; nothing closes a connection yet.
- **What closes it:** the host's outbox and the existing close path (FL-12).

### Clock sampling and the host entry

The decisions read the injected clock inside `transact`, and the stand-in's
`before_next_decision` hook proves a wait counts. Proving the real host's
post-wait sampling barrier belongs to the core/host callback entry, when that entry is in
a slice.

## Not established here

- No SQL, persistence, restart or host run: no Tokio or workerd code calls these
  decisions yet.
- Disabling the client on the sign-in side, removing sessions at revocation, and the real
  close.
