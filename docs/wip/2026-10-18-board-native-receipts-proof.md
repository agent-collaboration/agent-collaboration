# Native receipt consumer checkpoint

This checkpoint continues `e9f2e03c` through the separately credited B port
`77c8e3dcd1c6715654f76494570779a386179e6a` and the native storage/service delta.
It is a compile/integration checkpoint for B's restack, not completion or
acceptance of record bounds, public receipt reads, A5, or A6.

## Changes

- Project Accepted ledgers retain the bounded count/digest header. Immutable
  historical request-owned feed metadata supplies ordered effect pairs; the
  `(request_id, position)` index supports bounded 256-entry keyset pages with
  one-row lookahead. Repeated references at different positions remain distinct.
- Native Fleet admission stores ordered receipt rows with UUIDv7 keys and unique
  request/ordinal, for Changed and AlreadySo alike. Replay does not reconstruct
  historical bodies from the prunable feed. Native pages read retained receipts.
- Storage rejects appending an effect under an already decided request. Inserting
  an Accepted project ledger verifies its complete commitment. Rejected moves
  can still write incidental movement/notification records and expose no pages.
- Shared caller-field validation runs at admission and authoritative decode.
  Rejection vectors follow the adopted producer shape: at most eight guard and
  four other move reasons; other producers return exactly one.
- Internal intent admission returns the original intent transition's exact
  cursor under its claim request, independently of the public first page.
- Historical reader-metadata joins use request-owned feed rows. Runtime/test
  consumers read pair pages and tail headers. Positive client/MCP fixtures and
  the generated success-schema snapshot use the hard-cutover page shape.
- All ten current agent-fleet-sync manifest edges pin
  `40efe0233b429dccbd6d85c5fc645592d2112e2c`. The root lock changes fifteen Git
  identities and adds the legacy core dependency; unrelated Windows selections
  remain at their baseline. The nested Cloudflare lock adds 21 required packages
  for B's durable-steps dependency, with no existing package version changed.

## B's legacy adapter port

The Lead supplied the exact six-path patch from B's `7d435dfc`. Its SHA256 was
verified, it applied without local edits, and all six committed files match the
applied hashes at `77c8e3dc`. No reference-only B1 driver/test source was applied.

The sole subsequent necessary adaptation is
`tests/registry_support/session_driver.rs`: `service(1)` returns to `machine(1)`.
This layer's `ObjectAddress::Connection` still takes `InstallationId`; the
ServiceId address and helper belong to B1. The Lead accepted this adaptation.

The prescribed conformance command was:

```sh
cargo test --locked --offline -p collaboration-central-core --features conformance --test fleet_requests --test fleet_feed -- --nocapture
```

The adapted positive and exact-restored runs each passed `fleet_feed` 5/0/0 and
`fleet_requests` 5/0/0, exit 0. A compiled control skipped only the legacy
DecisionReceipt insertion. With `--no-fail-fast` to observe both suites, feed
passed 5/0 and requests passed 4/1, exit 101. The intended replay assertion
`decided_replay_and_conflicting_request_preserve_every_fleet_row_and_feed_entry`
reported an invalid/incomplete Fleet snapshot. The original source was restored
byte-exact (SHA256 `49e185569ca8158a0af391b287e01dd20e4a175b3b1b6df93c41495af9e2b606`).
No test was added for this control. B's modern checked-scope/workerd observations
remain B's evidence; they were not run on this pre-B1 layer.

## Native observations before the pin change

At `01d846b`, the focused receipt run passed 6/0/0 and the full native storage
library suite passed 58/0 with one intentional child-entry ignore, exit 0. All
3,883 source hashes matched. These results are qualified to that earlier pin.

- 513 historical project versions, interleaved with foreign request positions,
  returned pages of 256/256/1 after a later update and reopen. Foreign, ledger
  and wrong-family cursors were refused. Empty Changed/AlreadySo and Rejected
  stayed distinct; mismatched commitment and padded stored origin were refused.
- A real WorkspaceArchive over 1,001 boards produced 1,002 effects, returned as
  256/256/256/234 after explicit fixture deletion of feed rows and owner reopen.
  This proves retained receipt behavior, not execution of the pruning timer.
  No-op receipts retained their original pair after rename/pruning/reopen.
- The corrected heavy-case probe wrote genuine request-tagged versions first:
  accepted ledger rows for 1,000/10,000/20,000 effects measured
  31,968/31,971/31,971 bytes. No backlog or effect-count gate was introduced.
- Combined repeated maximum metadata measured receipt 872,424; accepted Move
  753,997; rejected Move 1,001,092; rejected ledger 279,300; trail 902,138; post
  357,731; description 146,434 bytes. Reconstructed schema was 12,460 bytes over
  48 statements. The rejected Move leaves 47,484 bytes below 1 MiB.
- Two permanent rejection-shape cases compiled and failed their intended old
  admission assertions, 0/2, exit 101. Both later passed admission and corrupt-row
  decode checks within the full storage suite.

The first heavy-case rerun exposed a fixture selector matching the request-ID
column on prerequisite rows. It now selects the actual FeedContent record
identity and keeps the exact content/codec assertions.

## Consumer repairs retained

`FleetReply.records` was retired by explicit decision. Its tests retain body
checks through owning reads rather than dropping coverage:

- `native_fleet_replays_original_record_and_decision_after_rename_and_reopen`:
  original receipt versus historical body/feed decoding and corruption.
- `repository_attachments_travel_in_workspace_feed_cache_and_reopen`: pair,
  owning record, feed, cache and reopen.
- `stand_in_reply_cache_and_pending_outbox_survive_restart`: pair, exact input
  body fields, reopened body and pending outbox identity.
- `workspace_identity_and_metadata_survive_reopen`: pair, renamed body fields
  and an additional owner reopen.

## Current checkpoint qualification and limits

The service/tests check on `40efe023` passed, exit 0, with all 3,883 source hashes
unchanged. The real MCP success-schema generator passed 1/0, exit 0, changing
only its golden file. The combined
`cargo check --workspace --tests --locked --offline` passed, exit 0, in 1m32s;
all 3,884 source hashes matched. The machine-wide slot was returned immediately.
This checks the whole workspace's tests for compilation; it does not execute
them. The carrying commit is qualified to this observed source manifest, with
no claim of a fresh immutable-head execution.

Public Router signed continuation projection, terminal-void re-evaluation,
public request_effects bindings, all-25-kind maximum proof, current-pin full
runtime/quality qualification, A5 ingestion/upload/RC-4, A6 and multi-Host or
hosted proof remain open. The raw native/session keyset is deliberately distinct
from the subsequently adopted opaque signed public continuation.
