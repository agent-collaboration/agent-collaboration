# Rejected-tail declarations checkpoint

This checkpoint gives central the shared tail header, membership chunk and read
cursor contracts. Native tail inspection, upload, storage completion, holder
ingestion and RC-4 behavior remain unfinished. It is not an A5 acceptance claim.

PD1 E29 and PD3 section 6 at `db944150` govern this checkpoint. The cutoff is a
board FeedCursor, derived read-only from the library cutoff through existing
capture bookkeeping. Its epoch need not equal the rejected epoch. No native
capture position is added to the report.

## Types

- `RejectedTailHeader` is the immutable commitment: project, epoch, cutoff,
  record digest, distinct member count and membership digest.
- `TailMembershipDigest` consumes a canonical ascending unique storage cursor
  without retaining the full list. It hashes the canonical JSON sequence of
  UUID request IDs. `RejectedTailHeader::from_requests` sorts and deduplicates
  supplied IDs before computing that commitment.
- `RejectedTailReportHeader::{Incomplete, Complete}` keeps an answering store's
  completeness observation separate from the immutable commitment. An upload's
  sender observation never proves completeness in the receiving store.
- `RejectedTailPage` has private validated header and request fields. It permits
  at most 256 ordered IDs; an incomplete header carries none. Overlapping or
  repeated deliveries must still be counted by distinct stored rows at the
  storage owner, not by the page length.
- `RejectedTailReadCursor::{Members, NextEpoch}` supports empty reports and
  movement across epochs. `RejectedTailReadPage` validates page, continuation
  and asOf together: an incomplete report cannot continue through members, and
  a continuation must name that report's epoch and last member where applicable.
- `BoardReadPage::TailReports` carries this validated payload. Its wire shape
  remains `{page, next, asOf}`. Upload uses the same membership chunk type.
  `Progress{stored}` is distinct from atomic `Stored{replayed}`.
- The shared board protocol also declares Fleet request-effects reads and their
  typed result, using the decision contracts from `01fbfbca`.

The two private wire structs route decoding through validated constructors.
There are no production lint exemptions. Test helpers permit unwrap solely for
fixed domain fixture construction.

## Observed proof

All-target tests for message-board, fleet-admission and board-sync-protocol
passed 149/0/0, exit 0, across 35 targets (34 nonempty). The same frozen source
passed all-target Clippy with warnings denied, compilation of all three
libraries for wasm32-unknown-unknown, Cargo format and Git diff checks. All
3,877 non-proof source hashes matched before and after the gates.

The five tail-domain cases cover independent literal digests for empty and
two-member sets; duplicate removal and order; rejection of an unordered hashing
cursor without changing its state; inherited cutoff epochs; a maximum-scalar
256-ID chunk encoding below 16,384 bytes; the 257-ID refusal; incomplete and
empty reports advancing epochs; and a complete sender reporting one partial
upload chunk. Protocol cases use the real library frame codec for the new
upload, progress, read, cursor and request-effects variants. Legacy flat tail
uploads are refused, and the malformed-project case still reaches its nested
identity boundary.

No pre-implementation behavioral red is claimed for these new types. After
implementation, a compiled control raised the tail-page bound to 257. The
literal 257-ID rejection assertion failed 0/1, exit 101. Exact byte restoration
then passed all five tail cases, exit 0. This is a sensitivity control, not a
pre-existing regression claim.

The invocation is parent `01fbfbca` plus the scoped tail declarations. The
carrying commit is qualified by matching its selected source files to that
observed manifest; no fresh immutable-head run is claimed after commit.

## Still unverified

The cutoff mapping must be exercised through retained native storage and must
equal the Takeover fact's last_replicated value in that proof. Atomic completion,
conflicting or overlapping page ingestion, notices only after completion,
read-only old-holder handling, current-holder voiding and fresh retry behavior
remain A5 integration work. Central and session continuation belong to B.
Receipt storage/public bindings, the all-record maximum proof, A6, two-Host
operation, CI and delivery acceptance remain open. No independent observation
of this checkpoint is claimed.
