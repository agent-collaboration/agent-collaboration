# B2 central on the D1 Fleet store

Working record for B2a–B2e on the central side. It supersedes the earlier
notes in this folder where they disagree:
`2026-11-05-b2-provisioning-link.md`'s organization-only body and
`2026-11-05-b2-organization-addressing.md`'s Copy routing hold.

## Decisions taken in this branch

- **Migrations.** B2's two tables are versions 11 and 12 of the
  `central-fleet` corpus, in their own directory
  (`organization_migrations/`), following `admin_import_migrations/0010`.
  The reset numbers that corpus with sequential integers; no branch uses 11
  or higher. The Copy's private note is version 2 of the
  `central-copy-summary` corpus (`copy_migrations/0002`).
- **Tables.** `organization_identity` and `organization_change_records` are
  the owner-approved SQL. `_copy_fleet_reference` is a table in the Copy's
  own SQLite on both hosts (the decision page allowed a table on
  Cloudflare too), so one code path serves both.
- **The Copy note grants nothing.** It routes the Copy's own follow-up calls
  to its organization's Fleet after a restart. A Copy without a note admits
  calls as D1 did, since ownership is decided by the caller's Fleet
  directory; a note naming another organization refuses the call. Only a
  creation or seed admission for an uninitialized Copy writes it. This
  replaces the in-memory stand-in whose admission gate broke 48 D1 Copy
  cases.
- **Provisioning body.** `{organizationId, organizationStatement}` (the B2
  doc governs over F1c's organization-only body). Renewal uses the same
  route with a statement carrying a signed `replaces` digest; the swap is a
  compare against the stored statement inside the applying transaction.
  Replaying the stored statement answers Provisioned and writes nothing. A
  renewal record's `request_id` is the digest of the statement it installs.
- **Statement lifetime.** 120 days (owner choice 3f). Central refuses a
  statement whose `exp − iat` exceeds 10,368,000 seconds. No central has ever
  stored a stamp (no prior code creates the table), so no one-year stored
  statement needs a transition.
- **Renewal read.** `{digest, expiresAt, signingKeyId}`, all taken from the
  stored statement as central verifies it for that read. The statement never
  crosses the link.
- **Stamp check.** A Fleet object checks its stored statement against the
  sign-in keys the first time it needs it after a start, and rechecks expiry
  on its injected clock at every use. A statement that fails refuses every
  command on every face except the link's provisioning and digest calls; a
  setup retry over it is refused (409) so sign-in never marks it ready, and
  only a replacement naming it is accepted.

## The private link

- **Tokio:** the link router is served on its own owner-only Unix socket;
  the public TCP listener never mounts it.
- **Cloudflare: a second Worker, `central-link`** (provisional, staging
  only, awaiting the owner's confirmation of shape). Reason: central is one
  workers-rs Worker with a single default `fetch` handler, and Cloudflare
  documents no reliable in-handler signal that a request arrived over a
  service binding rather than the internet (`request.cf` absence and
  `CF-Connecting-IP` are undocumented for this). workers-rs at the pinned
  revision generates one default entrypoint, so a named entrypoint would need
  a hand-written JS module. Cloudflare's documented pattern for an
  internal-only surface is a Worker with no routes and `workers_dev = false`,
  reachable only by a binding. `central-link` is the same crate built with
  the `link` feature; it mounts only the two link routes and reaches the
  Fleet objects through central's typed location policy and transport.
- **Reverting to a shared-secret check on one Worker** would take: drop the
  `link` feature entry, mount the link router on central behind a header
  check against a secret provisioned to both Workers, and keep the public
  404 for every other `/internal/*` path.
- Public central answers 404 for every `/internal/*` path before any
  credential check, on both hosts.

## Proof status

See the commit messages for each slice's commands, counts and what ran
where. Not yet proven: the Cloudflare link through a real caller Worker's
service binding, both hosts' first-provision race under an in-transaction
barrier, and the conformance record fault through a host.
