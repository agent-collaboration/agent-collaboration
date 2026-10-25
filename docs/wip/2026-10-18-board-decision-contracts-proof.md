# Board decision declarations checkpoint

This is a shared-domain checkpoint requested by the Lead so central can consume
the new contracts. Storage pages, public request-effects bindings, A5 tail types,
whole-workspace compilation and receipt integration remain unfinished. It is not
an integration or acceptance handback.

## Contract

PD1 E29 at `ecd4888d` governs the receipt layout. The key-error correction is
`37f3eacd`. The backlog gate and per-request effect cap were withdrawn before
implementation.

- `StoredDecision::Accepted` keeps `change` and a bounded
  `DecisionEffectsHeader { count, digest }`.
- `RequestDecision::Accepted` keeps `change` and `DecisionEffectsPage`:
  at most 256 ordered `(RecordRef, RecordPlacement)` entries, `totalCount`,
  the complete receipt's canonical SHA-256 digest and an optional `next` cursor.
- `RequestEffectsScope` distinguishes project and workspace Fleet authority;
  `RequestEffectsCursor` distinguishes project position and Fleet ordinal.
  `RequestEffectsRead` returns the decision or the adopted unknown-request,
  wrong-scope and invalid-cursor refusals.
- Fleet's pure write plan carries one `DecisionReceipt` per ordered pair.
  The adapter allocates its internal UUIDv7 row key. Fleet no-op pairs remain
  represented independently of feed writes.
- `AbridgedEvidence` is reserved for refusal evidence. It retains 256 ordered
  entries, the full count and digest, and an explicit abridgement flag. A cycle
  prefix is not labeled a closed cycle; ambiguous assignment evidence is
  explicitly non-exhaustive when abridged.
- `SubtaskPlaceError` distinguishes invalid shape from a key longer than
  8,192 hex bytes. There is no recorded refusal variant without a production
  producer. `between` does not rebalance its bounds.

The checkpoint also carries the adopted shared caller-field validators used by
the pure Fleet rules. Their native admission/decode enforcement and complete
maximum-record sizing belong to the unfinished integration slice.

## Observed proof

The final complete domain run passed 130 tests across 31 nonempty targets,
with zero failures or ignores, exit 0. The same frozen source passed:

- all-target Clippy for message-board and fleet-admission, warnings denied;
- compilation of both libraries for wasm32-unknown-unknown;
- Cargo format and Git diff checks.

All 3,875 non-proof source hashes matched before and after that command sequence.
The invocation is description parent `9583f339` plus the scoped declaration delta;
the carrying commit is qualified by matching its selected files to that source.
There is no claim of a fresh immutable-head run after the commit.

The focused tests cover a 600-effect receipt over 256/256/88 entries with exact
order, repeated references, count and digest; changed and already-so empty
receipts; rejected decisions retaining their status; wrong cursors and digests;
strict complete-page decoding; legacy wire rejection; full-rule refusal
abridgement; and exact 8,192/8,194-byte key behavior.

Observed reds, in order:

1. Before refusal abridgement, the new assignment-prefix test failed because
   the wire still carried an unbounded array.
2. Before the key cap, the ordering suite failed because 8,194 supplied hex
   bytes were accepted.
3. Before the ledger cutover, the native request-row test failed because its
   serialized decision had no effects header. Its integrated green is pending.
4. After the initial page implementation, a new complete-page decode test
   caught a mismatched digest. This was a defect in that working implementation,
   not a claim of a pre-existing product regression. The corrected domain suite
   passed it.

Two positive reply fixtures were repaired for the new page wrapper. Their
reader-state and input-order assertions remain. The independent empty-list
SHA-256 literal comes from encoding `[]`, not the production digest helper.
The first declaration Clippy passes caught unchecked indexing, then a test-helper
lint; these were corrected without relaxing production lints.

## Proof limits

The domain tests exercise real rule, constructor, serializer and pure Fleet
decision code. They do not establish native receipt persistence, continuation
reads through MCP/CLI, immutability after later transitions, central behavior,
two-Host operation, CI or delivery acceptance. The paired Verifier has not
observed this checkpoint. A5 tail declarations are not included yet.
