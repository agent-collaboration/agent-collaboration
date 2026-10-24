# Central corruption-to-retry conversion audit

Source-only audit requested by the Lead in handoff `01a155c9-314d`. Product
anchor: `f9caec484daf094f6ed395b7f5fc63fe4072a4d8`; library anchor:
`68394e062e143a133d46943ef23d01095ab5fe7b`. No failure classification was changed,
and no compiler, test or runtime was started for this audit. Directory and
typed-observer drafts were unbuilt when it was recorded; their later proof is
in the main implementation trace and does not repin this audit's source.

The scope is central's one-core `durable_steps::StoreError`, its native and
Worker wrappers, and the feature-gated probes. The superseded SqlTx error type
is separate and is not a durable CallFailure boundary. Searching all three
one-core runtime source trees found 212 CallFailure/classifier markers. I read
the source operations behind the storage-bearing conversions, and excluded
Serde/wire decoding, locks, timeouts, unknown faces, transport-only errors and
ID-source validation that cannot return StoreError::Corrupt. A generic storage
conversion listed here can erase a Corrupt returned by its owned store or
callback; this is not a claim that every input to that operation is corrupt or
that an infinite retry was executed. The private candidate inventory and
source-hashed site manifest are under `tmp/central-corruption-*`.

## Direct conversions

Paths in this table are relative to `crates/collaboration-central-core/src/`,
and line anchors are at the immutable product commit above. All 23 AnswerLost
sites below discard the typed store error. The peer lookup instead reports an
overloaded receiver. These are existing policies recorded for the Lead, not
instructions to replace them.

| Anchor | Store operation | Result |
| --- | --- | --- |
| `registry_session_calls.rs:89` | Session schema-report / Welcome step | `AnswerLost` |
| `fleet_archive_calls.rs:67` | Archive report step | `AnswerLost` |
| `fleet_claim_calls.rs:80` | Claim reservation / refused settlement step | `AnswerLost` |
| `fleet_runtime.rs:107` | Fleet receipt read step | `AnswerLost` |
| `registry_lease_calls.rs:74` | Session renewal step | `AnswerLost` |
| `session_revocation.rs:157` | Journaled end-sessions transaction | `AnswerLost` |
| `mail_status_receiver.rs:94` | Status publication transaction | `AnswerLost` |
| `mail_status_dispatch.rs:82` | Resolve owed status publication step | `AnswerLost` |
| `fleet_read_calls.rs:66` | Fleet snapshot / feed read step | `AnswerLost` |
| `fleet_metadata_calls.rs:92` | Canonical Fleet import step | `AnswerLost` |
| `fleet_metadata_calls.rs:132` | Canonical Fleet decision step | `AnswerLost` |
| `copy_directory_summary.rs:64` | Store copy summary step | `AnswerLost` |
| `fleet_lease_calls.rs:82` | Ordinary lease projection step | `AnswerLost` |
| `fleet_lease_calls.rs:104` | Advance Registry incarnation step | `AnswerLost` |
| `fleet_lease_calls.rs:128` | Apply projection and settle claim step | `AnswerLost` |
| `core_host/lease_settlement.rs:57` | Read public Registry snapshot for settlement | `AnswerLost` |
| `core_host/lease_settlement.rs:106` | Read last persisted incarnation | `AnswerLost` |
| `core_host/lease_settlement.rs:122` | Remember acquired incarnation | `AnswerLost` |
| `registry_mail_calls.rs:58` | Check mail origin / recipient enrollment step | `AnswerLost` |
| `object_transport.rs:175` | Owner revoke machine storage operation | `AnswerLost` |
| `object_transport.rs:182` | Owner enroll machine storage operation | `AnswerLost` |
| `object_transport.rs:191` | Machine authentication storage operation | `AnswerLost` |
| `mailbox_runtime.rs:323` | Admit / inspect mailbox lifecycle step | `AnswerLost` |
| `registry_peer_lookup.rs:43` | Read and validate peer-to-service enrollment mapping | `NotSent(Overloaded)` |

The 25th direct site is native `crates/collaboration-central-tokio/src/native_objects.rs:261`:
`objects.slot(address).await.map_err(|_| no_route())` maps all slot storage,
migration, bootstrap and initial-alarm errors to NotSent(Unreachable).
Its `no_route` is a caller-retry outcome. An unavailable/missing runtime branch
and `delivery()` face conversion at line 270 are separate non-store refusals.

## Indirect Copy and Mailbox classifications

- `copy_mutations.rs:66` classifies the StepDriver error and calls
  CopyTransactionFailure::call_failure. A bare StoreError::Corrupt falls through
  the library's `copy_failure.rs:148–184` into OutcomeUnknown(StoreBoundary), and
  `copy_failure.rs:49–55` turns that into AnswerLost. The same conversion serves
  a Copy status read at `copy_calls.rs:81` and the acknowledgement waiter at
  `copy_runtime.rs:123` (the method pointer to call_failure).
- Copy reads at `copy_read_calls.rs:76`, writes at `copy_write_calls.rs:257`,
  failure helper at `copy_write_calls.rs:450`, and failed discarded-part cleanup
  at `copy_write_calls.rs:267` use that same classifier but return a typed
  ReplicationAnswer::OutcomeUnknown(StoreBoundary) body. These are distinct
  from bare CallFailure and preserve the diagnostic. No new effect/no-effect
  classification is inferred from a bare store error.
- `mailbox_step.rs:94–106` classifies a failed mailbox step. A **bare**
  StoreError::Corrupt becomes DeliveryTransactionFailure::OutcomeUnknown in
  library `delivery_rollback.rs:55–82`, remains Unhandled in
  `mailbox_failure.rs:61`, and becomes AnswerLost in central line 106.
  This differs from the library's successful-rollback **custody-corrupt marker**:
  its recognized payload becomes DeliveryTransactionFailure::Corrupt, which
  `mailbox_failure.rs:53–59` returns as MailboxUnavailable(CustodyCorrupt) and
  central records in the in-memory health state. That typed branch is retained.

## Worker wrapper and later retries

Worker `object_shell.rs:59–63` and `:72–79` stringify initialize/alarm StoreErrors;
`:234` does the same for store migration. They are not themselves CallFailure
answers. A thrown/failed Worker fetch is reduced to AnswerLost by
`cloudflare_transport.rs:87`; unexpected status/body falls through at `:95`.
That is an additional loss boundary if corruption happens before an explicit
RPC result. These are source paths; no new Worker fault was executed.

The ordinary publisher in `core_host/lease_publication.rs:56–69` rearms a
five-second wake for every error, including corruption returned by its
publication store. The acquire failure at `:80–84` also becomes pending work.
These are background retries, not a correlated RPC answer. Copy follow-up
`core_host/copy_followup.rs:44` retries failed Fleet calls whose typed store error
has already been lost at the Fleet receiver; it is not a second direct store
conversion.

## Feature-gated observations

The following seven additional **conformance-only** storage/engine boundaries
also erase their store error into AnswerLost:

| Anchor under core/src | Operation |
| --- | --- |
| `conformance_holder.rs:231` | Admit holder probe |
| `conformance_holder.rs:235` | Drive holder probe journal |
| `conformance_holder.rs:287` | Read probe engine status |
| `conformance_holder.rs:298` | Read pending probe step |
| `conformance_copy.rs:263` | Admit Copy probe |
| `conformance_copy.rs:267` | Drive Copy probe journal |
| `machine_enrollment/conformance_revocation.rs:273` | Read revocation enrollment and journal state |

The Fleet receipt probe is a useful counterexample: `conformance_fleet.rs:92–95`
serializes the full Result<StepControlReply, StoreError>, preserving Corrupt.
Its serialization failure is a different error. Probe locks, signal waits,
dispatch-record memory and drive-token counters are not stored-record decoders.
The new unbuilt record observer similarly preserves the typed error plus its
RecordRef; it does not change the production transport policy.

## Available library outcome

Pinned `durable-steps/src/operation_model.rs:191–199` has exactly
CallFailure::{NotSent, AnswerLost, Retryable}. NotSentReason at `:225–230` has
Unreachable and Overloaded. There is **no non-retryable/fatal CallFailure**.
CallRetryReason's string code/detail at `:205–208` does not change Retryable into
a terminal outcome. The separate typed Call::Answer or an operation's terminal
OperationFailure can carry domain decisions, but is not a generic CallFailure
replacement. Choosing such a change belongs to the Lead/library design; this
audit introduces neither a new variant nor a special-case refusal.

Open proof: no deliberately corrupted production journal was driven repeatedly,
no retry duration was measured, and no terminal-corruption outcome was proposed
or accepted here. The existing Copy/Mailbox rollback-certainty rules must remain
part of any later decision.

## Lead disposition: decision 486

The Lead read this audit and chose **no behavior change in this stack**. A
non-retryable CallFailure requires a library contract change. Keep this audit
and list the inability to surface terminal storage corruption as a known limit
in PR 3's description. Copy/Mailbox rollback and effect-certainty classifications
remain unchanged. This is an owner disposition, not a runtime safety proof.
