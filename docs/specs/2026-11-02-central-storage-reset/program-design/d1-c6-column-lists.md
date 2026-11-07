# D1 C6: typed column lists for the remaining Fleet tables

Approved by the Lead on 2026-11-04, before the query rewrites. Authored by the Central Implementer from source at Central e6a8ce88d and SDK 277d6cb. These are realizations of the D1 table rules (UUIDv7 keys, INTEGER ms timestamps, no JSON columns, closed tags validated in Rust, no enum CHECKs) for tables that previously stored JSON records. They add no owner, store or authority. One addition beyond the earlier batches: `fleet_workspace_repository_refs`, an ordered child table that preserves a workspace's repository list exactly, because import keeps the offered order and the digest covers it.

---


Prepared at `e6a8ce88d`, SDK `277d6cb45a4a3eaa1156412a899788d71b901bc6`.
Proposal for the Lead to check and post on `d1.html` before query rewrites.
No schema or query changes have been made for these tables.

The existing owners stay the same: Fleet owns its immutable decision receipts
and admin import continuation; the project/receiver owns snapshot acceptance
and publication. Recording a header or phase grants no lease or Copy authority.

## Storage conventions

- UUID keys are canonical UUIDv7. Existing `receipt_id` and `import_id` remain
  the primary keys; the ordered manifest rows receive their own UUIDv7 keys.
- `created_at` and `updated_at` are SQLite INTEGER Unix milliseconds from the
  injected owner clock. Reads/replays keep the original creation time.
- Unbounded protocol u64 counters are canonical decimal TEXT, matching the
  approved lease/session convention; ordinal/count fields bounded by an
  in-memory list and the existing page limit are nonnegative INTEGER.
- Tags, nullable variant fields, timestamps and numeric ranges are validated in
  Rust, including their physical SQLite classes. No enum/range SQL CHECKs.
- No JSON columns in these three tables.

## `fleet_decision_receipts`

`DecisionEffect` is `(RecordRef, RecordPlacement)`.
`validate_fleet_decision_effect` requires Fleet placement and one of Workspace,
Board, Adr or Project. Receipt order and its original placement must survive
replay; the current resource row must not replace that historical evidence.

| Column | Storage and meaning |
| --- | --- |
| `receipt_id` | TEXT UUIDv7 PK, supplied by the existing owner ID source |
| `request_id` | TEXT UUIDv7 FK to `fleet_request_decisions(request_id)` |
| `ordinal` | INTEGER, exact zero-based effect order |
| `record_kind` | TEXT: `workspace`, `board`, `adr`, `project` |
| `record_id` | TEXT, the effect's typed resource identity; no polymorphic FK |
| `fleet_position` | INTEGER, the original Fleet placement |
| `origin_installation` | TEXT, the original placement's installation identity |
| `origin_machine` | TEXT, the original placement's machine label |
| `created_at`, `updated_at` | INTEGER; immutable receipts keep both unchanged |

Keep UNIQUE(`request_id`, `ordinal`). Replace `effect` JSON completely. Require
an exact contiguous ordinal range and the existing decision's count/digest
commitment on read. The effect's reference does not require a current resource
lookup or join, preserving the existing durable receipt contract.

Sources: `message-board/src/decision_effects.rs:9–29`,
`collaboration-central-core/src/fleet_decision_step.rs`.

## `fleet_admin_imports`

This is `AdminImportRecord`: immutable header plus Reserved → Published →
Complete. A replay returns the original `import_id` and header. Published and
Complete retain the exact publication position matching the manifest.

| Column | Storage and meaning |
| --- | --- |
| `import_id` | TEXT UUIDv7 PK, existing candidate/continuation identity |
| `project_id` | TEXT database identity, existing lookup key; preserve the current lack of a directory FK |
| `digest` | TEXT validated ImportDigest; existing semantic identity |
| `provenance_installation`, `provenance_machine` | TEXT, the original import provenance |
| `declared_owner` | TEXT HumanId |
| `manifest_epoch`, `manifest_sequence` | TEXT canonical u64 replication position |
| `schema_version` | TEXT canonical u64 |
| `schema_fingerprint` | TEXT, validated through SchemaIdentity |
| `pages` | INTEGER, existing nonzero u32 page count |
| `manifest_table_count` | INTEGER, expected ordered child-row count; detects a missing tail as well as gaps |
| `summary_kind` | TEXT: `project_summary` |
| `summary_time_basis` | TEXT: `router`, `legacy_unknown` |
| `summary_record_at` | nullable TEXT RFC3339; present only for Router time, preserving full precision |
| `phase_kind` | TEXT: `reserved`, `published`, `complete` |
| `published_epoch`, `published_sequence` | nullable TEXT canonical u64; both absent for Reserved, otherwise both present and equal to the manifest position |
| `created_at`, `updated_at` | INTEGER; publication/completion only advance updated time |

Keep UNIQUE(`project_id`, `digest`). Replace `header` and serialized `phase`
with these columns and the ordered child table below. This does not add a new
manifest uniqueness/acceptance rule at Fleet: the SDK receiver still decides
whether the offered snapshot can be installed.

Sources: `collaboration-central-core/src/admin_import_contract.rs:11–45`,
`admin_import_store.rs`, `fleet-admission/src/fleet_import_contract.rs:35–38`,
SDK `snapshot_manifest.rs:6–19` / `snapshot_page.rs:13–16`,
`board-sync-protocol/src/board_payloads.rs:50–52`.

## `fleet_admin_import_manifest_tables`

The manifest's ordered `Vec<TableManifest>` becomes child rows. Order is part
of header equality and must not be sorted or normalized during reconstruction.

| Column | Storage and meaning |
| --- | --- |
| `row_id` | TEXT UUIDv7 PK, owner-generated when the header is first reserved |
| `import_id` | TEXT UUIDv7 FK to `fleet_admin_imports(import_id)` |
| `ordinal` | INTEGER, original vector index |
| `table_name` | TEXT TableName |
| `row_count` | TEXT canonical u64, preserving values above i64::MAX |
| `table_digest` | TEXT TableDigest |
| `created_at`, `updated_at` | INTEGER; immutable header rows keep both unchanged |

UNIQUE(`import_id`, `ordinal`). Do not introduce UNIQUE(`import_id`,
`table_name`): the current continuation stores the offered header; adding a new
Fleet acceptance constraint would change the receiver's validation boundary.
Require ordinals 0..`manifest_table_count` exactly before constructing the SDK
manifest. Root plus children are inserted in the same existing Fleet scope;
phase transitions leave the header children untouched.

## Proof required after approval

Preserve exact first/replayed receipts and their digest/order through owning
reopen, original import ID/header/phase/publication through normal begin/page/
complete/reopen, and transaction rollback on a late write fault. Check fresh
schemas, UUIDv7 keys/native timestamps, u64 extremes, invalid tags/classes,
partial optional variants, missing/gapped/extra manifest children and unknown
stored values. Regenerate the existing checked SQL descriptions and run the
actual native/Worker routes affected by each rewrite.

Feed and request-ledger column lists are still being prepared; this is a partial
gate packet, not a claim that all C6 columns are approved or implemented.

---


Prepared at `e6a8ce88d`, SDK `277d6cb45a4a3eaa1156412a899788d71b901bc6`.
For the Lead's check and `d1.html` posting before query rewrites. No schema,
query, public record or project-table changes have been made for this packet.

Use the same UUIDv7 keys, INTEGER ingestion timestamps, canonical decimal TEXT
u64 counters and Rust validation conventions as the receipt/import packet.
Domain RFC3339 times keep their precision. JSON is removed from these tables.

## Common record envelope columns

For feed and ledger rows: `workspace_id`, nullable `board_id`, `actor_kind`,
nullable `actor_human_id`, `actor_service_id`, `actor_endpoint_id`,
`actor_session_id`, `actor_router_component`; nullable `envelope_request_id`;
`fleet_position`; nullable `router_time` (RFC3339), `time_basis`;
`origin_installation`, `origin_machine`; `created_at`, `updated_at` (INTEGER ms).

The actor fields are a checked tagged union of Human, Session and Router.
`actor_service_id` holds the Session endpoint's service or Router installation,
never an inferred peer id. Fleet records have no project scope. The envelope ID
is the typed record ID, as required by the existing `validate_record` /
`validate_ledger`; it is not another independently writable identity.

## Canonical workspace list addendum to batch 2

The approved workspace columns omitted its `repositories: Vec<RepositoryRef>`.
Normal attachment sorts the vector, but import deliberately clones the offered
order, and the record bounds check only caps it at 16. The semantic import
digest includes that vector. Reconstructing it from the unique attachment
lookup index would lose order and duplicates accepted by today's record type.

Preserve the field with `repository_count INTEGER` on `fleet_workspaces`, plus:

### `fleet_workspace_repository_refs`

`row_id` UUIDv7 PK; `workspace_id` FK `fleet_workspaces`; `ordinal` INTEGER;
`reference_kind` (`origin`, `local`); nullable `normalized_origin`, `service_id`,
`common_directory`; `created_at`, `updated_at` INTEGER ms.
UNIQUE(`workspace_id`, `ordinal`). Do not add repository-value uniqueness here.
Require exactly the parent's count and contiguous ordinals on read.

This replaces one field of the canonical Workspace record. The existing
`fleet_repository_attachments` table remains the association/provenance lookup
index with its approved business-key uniqueness. Fleet admission updates the
record/list and association effects in its existing atomic scope; a later
workspace rename must not invent a new attachment provenance.

Sources: `message-board/src/board_metadata.rs:134–140`,
`record_bounds.rs:85`, `fleet-admission/src/fleet_metadata_rules.rs:280–306`,
`fleet_import_rules.rs:240–253`, `fleet_import_contract.rs:60–82`,
`collaboration-central-core/src/fleet_repository_store.rs`.

## `fleet_feed`

Keep `entry_id` UUIDv7 PK and UNIQUE `position` INTEGER. Add the common record
envelope columns except a second `fleet_position`: the one physical `position`
reconstructs both the entry and its envelope, as their existing invariant
requires. Add these snapshot columns:

| Columns | Meaning |
| --- | --- |
| `record_kind`, `record_id` | Closed Fleet vocabulary: Workspace, Board, Adr, DirectoryEntry (wire reference kind Project), Request |
| `as_of` | canonical RFC3339 retention clock, preserving the current sub-millisecond boundary semantics |
| `name`, `description`, `workspace_state`, `repository_count` | Workspace snapshot fields |
| `board_kind`, `initiative_goal`, `initiative_done_when`, `board_state` | Board snapshot, using `name` / `description` and common workspace/board IDs |
| `project_title` | DirectoryEntry snapshot, with record ID and board ID from the common fields |
| `adr_title`, `adr_text`, `adr_status`, `supersedes_adr_id`, `made_in_kind`, `made_in_record_id`, `adr_revision`, `repository_file_count` | ADR snapshot; revision is canonical u64 TEXT |
| `ledger_request_id` | only for Request snapshots, FK to the immutable `fleet_request_decisions` row |

Only the active record kind's fields may be set. No join to mutable Workspace,
Board, Directory or ADR rows reconstructs historical feed content. Those bodies
and their ordered lists are snapshots taken during the existing append scope.
The DirectoryEntry public record currently contains ID, board and title; the
approved canonical directory's lease/copy columns do not become new public feed
fields in this storage rewrite.

Request decisions are immutable first-decision authorities, retained beyond the
feed's 30-day lifetime. A Request feed row references that same authority rather
than duplicating its decision/reason tree. Its record ID, complete envelope and
Fleet position must match the ledger row exactly. The read remains in the Fleet
store and returns the same self-contained public Request record; it never asks
the Registry/project or a mutable projection for history.

### `fleet_feed_repository_refs`

`row_id` UUIDv7 PK; `entry_id` FK `fleet_feed`; `ordinal` INTEGER;
`reference_kind`; nullable `normalized_origin`, `service_id`, `common_directory`;
`created_at`, `updated_at` INTEGER ms. UNIQUE(`entry_id`, `ordinal`).
Preserve the Workspace snapshot vector's order and duplicates; require exactly
`repository_count` contiguous rows. No business-key uniqueness in this snapshot.

### `fleet_feed_adr_repository_files`

`row_id` UUIDv7 PK; `entry_id` FK `fleet_feed`; `ordinal` INTEGER;
`repository_origin`, `path`, nullable `commit_sha`; `created_at`, `updated_at`
INTEGER ms. UNIQUE(`entry_id`, `ordinal`). Require exactly the ADR snapshot's
`repository_file_count` contiguous rows. Historical files must survive later ADR
revisions unchanged.

Feed retention removes its snapshot children and advances the existing floor in
the same owner scope. It does not remove request ledger authorities or canonical
directory rows. The already typed `fleet_feed_state` / readers are unchanged.

Sources: `fleet-admission/src/fleet_decision.rs:55–59`,
`message-board/src/project_feed.rs:15–65`, `board_metadata.rs`,
`workspace_adrs.rs:21–31`, `collaboration-central-core/src/fleet_record_codec.rs`
and `fleet_feed_store.rs`.

## `fleet_request_decisions`

`request_id` UUIDv7 PK; `canonical_hash`; common envelope columns;
`decision_kind` (`accepted`, `rejected`); nullable `change_kind` (`changed`,
`already_so`), `effects_count` (canonical u64 TEXT), `effects_digest`;
`reason_count` INTEGER. The common timestamp columns occur once.

Accepted has its exact effects header and no reason rows. Rejected has its exact
ordered reason tree and no effect header or receipt rows. The existing owner
checks the receipt count/digest. Preserve `envelope_request_id` independently of
the PK: the envelope's optional request link is a distinct field. The existing
Fleet invariant fixes `accepted_in = None` and project scope = None; these are
reconstructed constants, not new writable columns.

The current storage validator accepts the shared `Refusal` type, including
nested ProjectRejected and bounded evidence. Preserve that full declared shape;
do not narrow storage merely because today's Fleet writers usually produce
AlreadyExists, NotFound, NameConflict, Archived, Closed, InvalidShape or
StaleRevision. This is Fleet ledger storage, not new project operations.

### `fleet_decision_reasons`

| Columns | Meaning |
| --- | --- |
| `reason_id` UUIDv7 PK, `request_id` FK ledger, nullable `parent_reason_id` FK reasons, `ordinal` INTEGER | Ordered root reasons; ProjectRejected owns exactly one child at ordinal 0 |
| `refusal_group`, `refusal_kind` | Root, Workstream, Task, Trail, Role or Thread and their existing closed variant tag |
| nullable `record_kind`, `record_id` | NotFound, AlreadyExists, Closed and NameConflict references |
| nullable `name`, `field`, `current_revision`, `retry_after_seconds` | Exact scalar refusal payloads; unsigned counters use canonical decimal TEXT |
| nullable `workstream_id`, `task_id`, `thread_id` | Variant-specific subject IDs |
| nullable `requirement_kind`, `requirement_role` | Holds(role), LeadOrWorkstreamImplementer, AnyProjectRole |
| nullable `role_subject_kind`, `role_subject_id`, `authority_role` | Existing Project/Workstream role refusal payload |
| nullable `actor_kind`, `actor_human_id`, `actor_service_id`, `actor_endpoint_id`, `actor_session_id` | Thread refusal actor Identity |
| nullable `holder_kind`, `holder_human_id`, `holder_service_id`, `holder_endpoint_id`, `holder_session_id` | Current holder / pair-not-distinct Identity |
| nullable `target_kind`, `target_human_id`, `target_service_id`, `target_endpoint_id`, `target_session_id` | Handover target Identity |
| nullable `named_holder_kind`, `named_holder_human_id`, `named_holder_service_id`, `named_holder_endpoint_id`, `named_holder_session_id` | Stale-seat named holder Identity |
| nullable `last_seen_epoch`, `last_seen_position` | Exact optional FeedCursor; canonical u64 TEXT |
| nullable `evidence_kind`, `evidence_returned_count`, `evidence_total_count`, `evidence_digest`, `evidence_abridged` | Task-cycle or eligible-assignment AbridgedEvidence commitment |
| `created_at`, `updated_at` INTEGER ms | Immutable decision child timestamps |

Root uniqueness is a partial UNIQUE(`request_id`, `ordinal`) WHERE
`parent_reason_id IS NULL`; child uniqueness is UNIQUE(`parent_reason_id`,
`ordinal`). Require parents to belong to the same request. Rust rejects cycles,
unreachable nodes, missing/extra roots or children, unknown tags and unused
variant fields. It reuses the shared types' exact validation; in particular the
Thread holder/last-seen coupling must not be generalized to variants that do
not impose it today.

### `fleet_decision_reason_evidence`

`row_id` UUIDv7 PK; `reason_id` FK `fleet_decision_reasons`; `ordinal` INTEGER;
`entry_kind` (`task`, `assignment`); nullable `task_id`, `assignment_kind`,
`assignment_project_id`, `assignment_workstream_id`; `created_at`, `updated_at`
INTEGER ms. UNIQUE(`reason_id`, `ordinal`).

Assignment kind is the existing Lead/Advisor/Implementer/Verifier/Reviewer/
Operator variant; it determines which typed subject field is present. Evidence
entries remain ordered and bounded at 256. Preserve the parent's full count,
digest and abridged flag instead of recomputing the full commitment from a
truncated prefix. `evidence_abridged` is an optional INTEGER boolean; any SQL
CHECK on it is only 0/1. All other variant/range rules remain Rust validation.

Sources: `message-board/src/project_feed.rs:240–247`,
`request_decisions.rs`, `decision_effects.rs`, `abridged_evidence.rs`,
`authority_roles.rs:171–204`, `board_identity.rs:220`, `thread_refusals.rs`,
`collaboration-central-core/src/fleet_decision_step.rs`.

## Proof after approval

Read typed snapshots after a later canonical metadata/lease change and physical
reopen, preserving old actor/time/origin/content/list order. Exercise imported
Workspace lists with nonsorted/duplicate refs, ADR files, all accepted/rejected
ledger shapes, nested refusals and complete/abridged evidence. Preserve exact
effect count/digest/order and canonical request replay/conflict. Reject missing
tail/gapped/extra children, cross-request parents, cycles, invalid physical
classes, enum tags and nullable variant combinations. Late write faults must
roll back parent/children/feed head together. Regenerate checked queries and run
the affected native/Worker flows; the public protocol shape stays unchanged.

---

## Addendum (2026-11-04): `fleet_workspace_adrs.repository_file_count`

Add `repository_file_count INTEGER NOT NULL` to `fleet_workspace_adrs`. It's the count of the ADR's ordered `fleet_adr_repository_files` child rows. Like the workspace's `repository_count`, it lets a read detect a missing tail, which ordinals alone can't, because the source `Adr.repository_files` is a `Vec` capped at 16 (`record_bounds.rs:93–97`). It's validated in Rust: nonnegative, at most 16, equal to the exact child count, with contiguous ordinals. No SQL CHECK, and no change to authority or the public contract.
