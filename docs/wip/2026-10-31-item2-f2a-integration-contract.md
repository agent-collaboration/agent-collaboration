# F2a organization authority integration contract

This records the F2a/F2c/F3a integration boundary before implementation. The
item-two Program Design and delivery plan govern behavior. F1's existing Rust
contracts and F2c's new directory contracts own the body/result types; this
document does not replace them. The 1 November ruling supersedes the earlier
Registry and F2b allocation. Storage, handlers and host routing described here
remain to be implemented; the current code contains the person-proof readers
and pure authorization policy.

F2a owns the organization authority, its store and transaction rules, token
parsing/policy, guarded admin handlers, admissions, allowances and session
registry. F3a supplies real signature verification, sign-in key fetching and
caching, authenticated session confirmation, and mounting the bearer routes.
F2c owns the separate Fleet directory, its organization list, organization
creation and initialization outbox, and the central deploy setting naming its
one organization. Each central retains its existing single Fleet object and
store; F2b was dropped by the 1 November ruling (R55).

## Person verification and confirmation

The authority accepts a bearer token through its handler. It parses an OAuth
access token, asks the injected signature verifier about that exact signed
input, applies the configured issuer/audience/client policy, and asks the
sign-in service to confirm that same token. No Access identity proves an
item-two owner or operator.

The runtime receives these typed capabilities:

| Capability | Input | Result |
| --- | --- | --- |
| Person signature verifier | Algorithm, key ID, exact JWT signing bytes and signature bytes; issuer/key source comes from pinned configuration | Valid, invalid, or unavailable |
| Person session confirmer | The same opaque bearer token | Active confirmation naming person, session, audience, expiry and organization; inactive; or unavailable |
| Clock | No input | Central's current time |
| ID source | No input | A fresh UUIDv7 |

An active confirmation must agree with the verified token's person, session,
audience and organization and remain unexpired. An unavailable or inactive
answer admits nothing. The provider's `checkedAt` is not the guard's clock.
The concrete K5/K6 wire readers are checked against the producer-owned schemas
and structural fixtures from B3; the real-output check belongs to the later
of F2a and B4a. Until then those producer interactions remain unverified.

The owner and operator audiences are separate configured resource identifiers.
Owner calls also require the token's organization to match the addressed
authority. The operator must match the configured fleet-operator person and
the pinned sign-in client; ownership of an organization grants no operator
authority.

Each invocation obtains its own confirmation. Its receipt time is sampled on
central's injected clock. The applying transaction checks the current owner
from its own store and the five-second receipt-to-apply bound. After the
bound, the request must obtain a fresh confirmation before it can apply.
Confirmations are not cached for later commands; no client-presentable
confirmation replay token or nonce registry is introduced.

Every owner mutation checks its second-factor timestamp against the single
900-second setting, including the boundary. Reads do not check second-factor
freshness. Writes use `CommandRefusal`, including `SecondFactorRequired`;
reads use `OwnerRefusal`, which has no such variant.

The handler first validates the caller and reads its current owner/standing,
then checks the factor for a mutation, then parses its operation-specific
body. Semantic body checks and external subject queries cannot disclose a
subject to an unauthorized caller. The mutation transaction rereads its
owner and standing after any external I/O.

## Admin and operator HTTP paths

These paths are recorded for F3a to mount. F2a does not mount an unauthenticated
or fixture-backed production route. Existing Access routes remain unchanged
until their planned cutover. All methods below are POST, with JSON request
and result bodies from `collaboration-central-core::identity_contract`.
Every body refuses unknown fields.

| Path | Body | Result |
| --- | --- | --- |
| `/v1/admin/organizations/{organizationId}/commands/create-workspace` | `CreateWorkspace` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/allow-sign-in` | `AllowSignIn` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/remove-sign-in` | `RemoveSignIn` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/allow-machine` | `AllowMachine` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/withdraw-machine-allowance` | `WithdrawMachineAllowance` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/remove-session` | `RemoveSession` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/commands/set-token-lifetime` | `SetTokenLifetime` | `OrganizationCommandResult` |
| `/v1/admin/organizations/{organizationId}/lookups/identity` | `IdentityLookup` | `IdentityLookupResult` |
| `/v1/admin/organizations/{organizationId}/lookups/workspace` | `WorkspaceLookup` | `WorkspaceLookupResult` |
| `/v1/admin/organizations/{organizationId}/lookups/machine` | `MachineLookup` | `MachineLookupResult` |
| `/v1/admin/organizations/{organizationId}/lookups/command-status` | `CommandStatus` | `CommandStatusResult` |
| `/v1/operator/organizations/{organizationId}/owner` | `SetOrganizationOwner` | `OrganizationCommandResult` |

The path selects a closed operation and organization before the body is
parsed. The body's organization must match it. No operation is selected from
an arbitrary JSON tag. F10 adds its enrollment-revocation handler; device-code
approval uses its own service edge, not an admin command. Presence and
WhoIsLoggedIn handlers belong to their later ticket.

`CreateOrganization` belongs to F2c's directory. F3a mounts that operator
receiver there. It is not one of F2a's central handlers.

## Fleet capability

The authority asks its injected Fleet capability to read a workspace or a
project in exactly one organization. An answer identifies that organization
and subject, or says unknown/unavailable. It never supplies the authority
with an organization-wide cache or access to another store's connection.

Workspace lookup returns F1's `WorkspaceRef`. Project lookup returns the
project and its workspace references needed by registration and identity
lookup. Unknown subjects produce the same bytes whether they exist in
another organization or nowhere. Unavailable storage is not interpreted as
an empty result.

For workspace creation the authority commits an admitted command, its audit
record and durable outbox admission in the same transaction. Its persisted
command ID is the idempotency key for `CreateWorkspaceOnce`, whose typed
input names organization, command ID, proposed workspace ID and canonical
name. The result is applied with the actual workspace reference, or
unavailable for retry. A canonical name already present resolves to that
workspace and marks this command applied once; distinct command IDs may
resolve to the same workspace. The Fleet owner records its own effects.

Canonical name means ResourceName's own trimmed value, compared byte for
byte. Comparison is case-sensitive and applies no Unicode normalization:
`Docs` and `docs` differ, as do NFC and NFD spellings of `é`; `  Docs ` and
`Docs` resolve to the same value.

The guard holds at admission. Losing ownership afterwards does not cancel
the admitted empty-workspace creation. An actual outbox restart must resend
the same command ID and preserve its pending/applied status.

## Object and storage ownership

Each central serves exactly the organization named in its deploy settings.
`ObjectAddress::Organization(OrganizationId)` is the planned address of its
authority, derived directly from that ID. Native hosting gives it its own
SQLite store; Cloudflare hosting gives it a distinct OrganizationObject.
F2a adds the authority's minimal routing and lifecycle. A request naming
another organization is refused before state changes; the central holds no
rows or authority for that organization (R34, R55).

F2c's separate directory owns organization list entries, their whole
`CentralRecord` and their creating/active state. Its creation transaction
records the creating entry and an outbox step. The step calls the named
central over the network; it does not call a sibling authority object.
F3a supplies the authenticated directory caller and mounts the central's
receiver, restricted to the directory's pinned client, audience and scope.

F2a owns the central-side initialization handler. Its shared request and
result types are defined by F2c under `identity_contract.rs`:

- `InitializeOrganization { organizationId, ownerEmail, commandId }`.
- `OrganizationInitialization { result: initialized | alreadyInitialized |
  notThisCentral }`.

The handler checks the request against the deployed organization first.
`notThisCentral` changes nothing. For the matching organization, it initializes
the owner email as an unbound entry and the authority record in one authority
transaction, and ensures that central's Fleet exists. Repeating initialization
returns `alreadyInitialized`. The directory marks its own entry active only
after acknowledgment. An uninitialized authority refuses commands, decisions
and sign-in from its own state, even while the directory lists it as creating.

The operator's owner replacement changes the authority's owner email and
binding. An existing bound allowlist entry for that email supplies the new
binding; otherwise the entry is unbound until its provider sign-in. The
previous owner keeps their admission as a member until removed. Repeating
the same owner assignment does not erase its established binding.

The exact SQL for F2a's nine authority tables remains held for owner approval.
The two Registry tables in the earlier proposal are superseded: F2c owns the
directory tables and their separate SQL approval. No commit may use an
unapproved migration. F2a does not drop existing machine
enrollments, move existing Fleet data, create a machine claim, or implement
F10's activation/token/revocation lifecycle.

## Scoped proof and open integration

Permanent shared conformance cases run on native Tokio and compiled wasm
under local workerd, through the existing `composed-transactions` selection.
They use the same bodies, assertions and case counts, controlled clocks,
injected UUIDv7 IDs and event-driven waits. No new CI step or scenario-list
edit is required. Workerd recovery means stopping and restarting the real
process against the same persistence directory.

The signature/confirmation and Fleet capabilities use contract fixtures in
these scenarios. They establish the authority's rules, real stored
transactions, rollback and restart behavior. They leave real C7
authentication, real sign-in output, directory-to-central initialization and
the Router registration/binding journey unverified until their owning tickets
land and their real interactions pass. F2c's two-central directory scenarios
prove creation and routing across deployments; they do not replace F2a's own
transaction, guard and restart proof.
