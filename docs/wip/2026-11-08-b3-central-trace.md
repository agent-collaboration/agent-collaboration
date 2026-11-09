# B3 central: machine login decisions — trace

B3 machine login in `collaboration-central-core`: the decisions, then (B3 central storage)
their staged tables, the switch for today's machines and one SQL adapter both hosts run.
The hosts inject the clock, ID source and store. The enrollment's stamp pin,
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
the mirror's 103 validation vectors (switchboard `de59e3c4`, in
`contracts/issuer-mirror/`), including 5 accepted and 3 refused change records. Central refuses
exactly the 62 the mirror refuses and accepts the 41 it accepts, writing each byte for byte. The one named exception is row 6's null
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
- **Proof:** for both entries and both bounds, held through `before_next_decision`: 1 ms before the bound is admitted or renewed; equality and 1 ms after are refused as token expiry, and the store compares equal to its state before (full typed `EnrollmentStore` equality). The timer is tested with each bound ending first and with a tie.
- **Seeded reds:**
  - with only the in-transaction recheck bypassed, the equality case is admitted or renewed;
  - with a raw-exp timer, or one without the grant, the timer test fails.

## Assumptions for the owner page

Each was added by an agent or ruled by the Lead unless it names an owner decision; the PD
doesn't state them.

- **`formerServiceLinkedElsewhere {}`** is a new bounded device-login refusal. It answers a
  re-key whose former service id is already linked to a different installation, or that
  would merge into an installation that already has rows here. It says nothing beyond
  "not linkable here". The switchboard B3a Implementer has the variant.
- **`allowedWorkspaces` holds at most 100 workspaces.** PD:881 states no bound. The issuer
  and central both enforce 100.
- **A repeated device-login request** keeps the full-request contract: the same body
  answers as before; a different body is refused `notPermitted`, nothing written. The
  in-memory stand-in proves it. **On the SQL store it is pending the owner**: no stored
  form of the prior request exists yet, so a repeat answers `RepeatPendingOwner` and writes
  nothing. That proof is placeholder only.
- **SD8** is measured from the host's receipt to the clock read in the applying
  transaction. Exactly 5 s is within.
- **"The organization's first machine"** (PD:882) means no enrollment in any state, ever.
- **The peer is the installation.** `DecideDeviceLogin` refuses `notPermitted` when they
  differ, and admission compares the presented peer with the installation.
- **An approval lapses at the code's own deadline (owner N10):** `now ≥ codeExpiresAt`,
  carried by `DecideDeviceLogin.codeExpiresAt`. It's stored as 'lapsed' with `ended_at`.
  The column `code_expires_at INTEGER NOT NULL` is agent-proposed, pending the owner's
  confirmation, so its real-store tests are proposed-schema evidence.
- **Idle expiry is recorded (owner N07):** a `machineExpired` record with origin
  `automatic` (no person, no permission), plus `allowanceChanged` per withdrawn allowance,
  written in the transaction that first observes the expiry. It doesn't move the
  generation.
- **Records:**
  - `machineEnrolled` is written at the first Grant, never at approval.
  - Supersession writes `machineRevoked` (reason superseded) in the same transaction.
  - Approvals and first Grants carry the approver with `machine:enroll`; revocation carries
    the revoker with `machine:revoke`; idle expiry is `automatic`.
- **Change-record key convention (Lead):** each record's `request_id` is
  `<originating id>:<kind>:<target id>`, built at the B3 decision boundary, so a retried
  decision writes the same keys and no two records of one decision share one. B2's keys
  are unchanged. Every part is a UUID, so a key fits B2's 128-character bound.
- **`RevocationBy.operation` is an `AdminOperationId` (UUIDv7)** (Lead, ratified): it's
  internal, has no wire field or production caller, and keeps every revocation's keys
  within 128 characters.
- **A former service id that names no installation here** links nothing; once set it is
  never replaced.
- **The renewal answer's mapping (owner N11 adds `unavailable`):**
  - a key-set fetch failure, a malformed published key set, or a store that is
    unavailable or whose outcome is unknown → `unavailable`;
  - an unknown kid, a forged or malformed token, or a crypto or claim failure →
    `tokenInvalid`; a token refused for time → `tokenExpired`;
  - another installation → `wrongInstallation`; another enrollment of the same machine,
    superseded, another organization or inactive → `enrollmentNotActive`.
- **B3's switch converts today's machines exactly (Lead, ratified):** each RFC 3339 time
  becomes exact milliseconds; an unparseable or finer-than-millisecond time refuses the
  whole switch, which rolls back with nothing changed. The refusal names every refused
  row by id (or position in id order when the id is invalid) and its offending columns,
  with no stored values. `approved_at` is `created_at`.

## B3 central storage (this slice)

- **Schema:** the two migrations are staged, not registered
  (`staged_registry_migrations/0003`, `0004`), because registering v3 would break today's
  readers. Only fixtures initialize them, until the B3 reader slice moves them into
  `registry_migrations/` and registers them. Production bootstrap is unchanged.
- **Adapter:** `SqlEnrollmentPort`, one generic adapter over `AtomicStore` and
  `TransactionScope`, with checked queries (`b3-registry-queries.json`,
  `b3-legacy-queries.json`) verified fresh in `build.rs`. It runs on sqlx-turso (Tokio)
  and Durable Object SQLite (workerd).
- **The switch:** v3 runs a Rust step around its SQL inside the migration transaction: it
  refuses while a legacy revocation or readmission is unfinished in the journal, converts
  every row, then inserts them after the DDL.
- **Both hosts (Lead ruling: production runs on a Durable Object, so DO is never the
  thinner side):** `sql_cases` holds every SQL case, 24, each on a fresh store and
  generic over one small `CaseHost` seam. Each step checks the store's full readback
  (installations, enrollments, allowances, change records, every column) against exactly
  the rows it may write; refusals check that nothing changed. Tokio runs each case as its
  own test; workerd runs them in `BootstrapObject` (fixture `machineLogin`, one named
  object per case) through `machine_login_cases_workerd`. No case is native-only.

## Stand-ins

### The repeat's prior request on the SQL store

- **Seam:** `EnrollmentFacts.prior_approval`.
- **Stand-in:** none on SQL; a repeat answers `RepeatPendingOwner`.
- **What closes it:** the owner's decision on how the prior request is stored.

### The grant lifetime

- **Seam:** `MachineLoginSettings::grant_lifetime`.
- **Stand-in:** a value the host supplies, until `SetTokenLifetime` has a home.
- **What closes it:** the organization's stored lifetime.

### The close instruction

- **Seam:** `MachineRevocation::close`.
- **Stand-in:** a returned value; nothing closes a connection yet.
- **What closes it:** the host's outbox and the existing close path (FL-12).

### Clock sampling and the host entry

The decisions read the injected clock inside `transact`. Proving the real host's
post-wait sampling barrier belongs to the core/host callback entry, when that entry is in
a slice.

## Not established here

- No production host calls these decisions or reads the new tables; the corpus is staged.
- Running the switch on any real database is the owner's step-2 action.
- Persistence on Tokio is an owning-store reopen in the same process, not a new
  process. On workerd it is a later request to the same named object in one workerd
  run; a workerd process restart is not observed.
- Disabling the client on the sign-in side, removing sessions at revocation, and the real
  close.
