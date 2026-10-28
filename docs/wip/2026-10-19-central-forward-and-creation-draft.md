# Central forwarding and creation continuation

Source-only draft after clean Notices checkpoint
`13b8ad56341cdddbac23063c497f30fa241d6fb2`. Verifier B holds the build/test/Worker
window under Lead 01a15612. The ForwardedPost tests below now compile and fail at their intended real
forwarding boundaries (0/2, exit101); the supplied regression is also 0/1.
The presence component is implemented with its separate 36-case selected
proof and three caught controls in the main trace. No forwarding pass is
claimed. Governing sources are the Lead's design
tree at `7059809739ae68a576801ac4cce5628f8568f9fd`, PD3 §2a and §5 Creation,
one-core §0a, decisions 469/470/502, and the later 501 proof clarification.

## Forwarding 476

The existing owners are `forward_rules.rs`, `forward_contract.rs` and
`forward_delivery.rs`. Their synchronous `SqlTx` callers still exist. The new
session entry is `CoreSessionScope::stage_frame_effects`; committed network work
leaves through `EffectDriver`. `CoreHost::classify_session` owns the typed
Session → Registry and Session → Delivery edges. No runtime routes the policy.

The modern machine protocol already carries correlation:

```text
sender ClientFrame::Request(frame, Consumer::ForwardedPost{project, post})
  → reserve pending correlation in sender's own store, deadline now + 30 s
  → Registry route read (project, verified sender session)
  → delivery face of the Registry-resolved holder installation
  → ServerFrame::Request(holder frame, Consumer::ForwardedPost(VerifiedForwardedPost))
  ← ClientFrame::Answer(holder frame, Consumer::ForwardedPost(BoardPayload))
  → sender delivery face validates source installation/project/phase/deadline
  → ServerFrame::Answer(original sender frame, ForwardOutcome::Answered{reply})
```

The holder's protocol request has no additional forward-ID field. Its server
frame identifies the delivered correlation. Keep the sender's original request
frame separate from that holder frame and preserve the exact logical session
and physical connection when accepting answers. A replacement transport must
not resolve an earlier holder frame. The existing holder reply hub is transient;
it does not replace the design's durable correlation rows or deadline cleanup.

The sender row holds IDs, project, sender session/connection, phase and deadline.
`Routing` has no holder; `AwaitingHolder` carries the expected ServiceId. The
holder row binds its server frame to the verified sending Connection address,
project, holder session/connection and deadline. Neither row contains a post or
a reply. `ActorOrigin` is transient provenance, never the address for the answer.
Its installation and machine label come from the Registry enrollment, never a
cast from PeerId or an untrusted post body.

Reserve/count all phases atomically, with the existing limit 16. Route completion
must re-read the row before delivery, preserve the original deadline, and ignore
missing, expired or already-advanced rows. Delivery with no current connection
returns NoConnection; the sender yields HolderUnreachable with the Registry's
presence. Other dispatch failures yield Unanswered and free the slot. Timeout
cleanup uses the object's existing alarm owner; no task stores the transient
post or reply for retry. Unknown, late or wrong-sender answers acknowledge and
produce no send. After sender replacement, remove its row but drop the stale
physical answer.

Two permanent native drafts now live in `tests/forwarded_posts.rs`, using real
HTTP admission, WebSockets, Registry/Copy stores and public grant setup:

- `a_forward_preserves_verified_origin_and_returns_the_holders_opaque_answer`:
  rejects a peer-as-ServiceId origin, a fabricated label, a dropped body, a lossy
  integer, wrong frame correlation or changed opaque reply. The expected origin
  is the literal enrolled v4 ServiceId and machine label; the expected post and
  reply are independent literal payloads. The public library registration setup
  has an explicit admission-gate stand-in; it does not prove normal Register.
- `a_disconnected_holder_is_reported_with_its_registry_presence`: a real stored
  grant and prior admission remain after server reopen, while the holder has no
  physical transport. It rejects a closed sender, another outcome family or
  fabricated absence of the observed presence.

The supplied unknown-project ForwardedPost regression remains unchanged and
previously red. No red is claimed for either new draft. The remaining real-store
cases must force the 16-slot race, timeout/reopen, replacement, duplicate/late
and wrong-sender answers, and inspect correlation/journal rows while a body and
reply are in flight. A source-string scan is not persistence proof.

## Presence 502

Lead 01a15617 binds nullable `machine_enrollments.last_seen`, encoded like
`expires_at` and strictly decoded into Option<SyncTime>. Amend the unreleased
0001 migration only while no installed ledger has applied it; otherwise use an
additive migration and report that environment. Known own proof stores were
temporary fixtures, not installations.

The Registry writes its own clock at successful verified Hello admission only.
Use `SessionRegistryStep`'s service/peer comparison and schema-report branch.
The public library `accepted_schema_support` and `report_schema_support` can
read/write in that same scope. A stale report returns SchemaSupportRecorded
without replacing the current report, so a late replay must not fabricate a new
presence time. Compare the accepted attach sequence, update only for a newly
accepted declaration, and keep this observation atomic with that declaration.
Do not update at renewals: the library's HolderStale.last_renewed_at is a distinct
fact. Forward routing and the later 473 holder-change adapter read this one
Registry-owned value. Required proof: None before admission, exact injected
Registry time at admission, no renewal change, stale-report behavior, and actual
disconnected-holder route readback.

## Creation 469, held at the shared snapshot boundary

The Router's local admission was removed, but the synchronous core still calls
`fleet_snapshot::read_snapshot`: `core_steps.rs:291` → `fleet_rules` →
`fleet_request_admission.rs:16`, plus `archive_receiver`. Lead 01a15617 confirms
that FleetReadKey/FleetSnapshot wait for the port lane integration and deletion
of LocalServer/SqlTx. No empty binding, legacy index, fallback admission or
second pure rule is introduced.

The native storage/read draft keeps one decision authority. A normalized index
can refer to the existing immutable ledger decision:

```sql
CREATE TABLE creation_decisions (
    project_id TEXT NOT NULL PRIMARY KEY,
    creating_request TEXT NOT NULL UNIQUE,
    carrier_installation TEXT NOT NULL,
    FOREIGN KEY(creating_request) REFERENCES fleet_request_decisions(request_id)
);
```

Lead 01a1561b asks for the retention anchor before binding this normalization.
At 13b8ad56, `fleet_decision_step.rs:82–95` returns a saved decision or conflict
before any write; `:115–123` uses the plain INSERT in
`sql/insert_fleet_decision.sql:1–2`, with no upsert. The ledger schema has no
expiry field. The thirty-day prune deletes only `fleet_feed`
(`sql/fleet/prune_fleet_feed.sql:1`). The complete table-reference inventory in
crates, scripts and .github contains no runtime ledger deletion/update. Migration
0003 rebuilds the table but copies every request ID, hash and decision payload
verbatim (`core_migrations/0003_fleet_records.sql:103–113`). Thus the current
source retains decisions indefinitely, including rejected ones. Lead 01a1561d binds this normalized shape as decision 503; its FK prevents
a future prune from deleting a bound ledger row. Add a real retention case that
expires the feed and verifies the original accepted/rejected creation binding
and terminal-ID refusal still stand.

`project_id` and `creating_request` retain their validated v7 domain IDs;
`carrier_installation` is the verified ServiceId, which may be v4. An indexed
leaf query joins `creating_request` to the immutable ledger payload rather than
maintaining a second copy of Accepted/Rejected. The returned internal shape is
`CreationBinding {project, creating_request, carrier: ServiceId,
decision: StoredDecision}`. Decode the existing ledger with
`fleet_decision_step::decode`, validate IDs and the bound request, and classify
a missing referenced ledger as Corrupt, never pending.

At the eventual write boundary, bind from the authenticated FleetMetadataRequest
and original ProjectDirectoryCreate request, in the same borrowed scope as
`persist_decision`. The first creation binds accepted or rejected. A replay
returns the existing decision, and a different request records its own
AlreadyExists refusal without modifying the first binding. This last rule is
the shared pure-snapshot change held above, not a central-only guard.

The Registry leaf adapter is `ReadCreationDecision`, over the existing
Registry → Fleet edge, with project as request and Option<CreationBinding> as
answer. Fleet performs only the checked read. At Registry, map the writer PeerId
through its own enrollment row and compare ServiceId with the returned carrier.
Matching Accepted admits; mismatched Accepted refuses creatorMismatch; Rejected
carries the directory's typed refusal code/detail; absent decision yields
GateFailure and leaves registration retryable. Keep the library's original
CallId. The pinned service adapter maps GateFailure to CallFailure::AnswerLost
(`fleet-sync-service/src/routed_calls.rs:84–91`); invent no retry-reason code.

After the legacy caller is gone, add the declared Creation read to the shared
snapshot and its pure rule. Then prove all four real Registry/Fleet gate
answers, accepted/rejected terminal IDs with the original row unchanged,
mutation/ledger/index rollback, answer loss and replay, and first-piece creation
through the normal session. None is established by this draft.

## Router recovery 501

At the next authorized restack, use A's PositionedBoardNotice/NoticeRead shapes
and Host reaction. The real FleetClient::synchronize replaces its stand-in.
For both expired and restored-older invalid cursors, change a Fleet record and
assert the Router cache actually reflects it after the reaction, set the notice
cursor to floor, and handle a subsequent notice exactly once. An Ok result alone
does not show refresh. The method's source doc comment now explains coalescing
under try_lock and connected-timer retry, as Lead 01a15614 requested. That
comment-only edit has no new behavior claim or runtime observation.

## Integration input and next order

Lead 01a1562c supplies accepted feat/central-local-port head 85d65abe, with its
33-row ledger and conformance controls on both runtimes. After the current
clean checkpoint/window release for immutable 13b8ad56, merge the port input
and keep our changes at conflicting owners. Declare that integrated head
before deleting LocalServer/SqlTx. Each still-owed d/e/f row needs its named
new home in the ledger; no deletion may turn it into passing evidence. The
469 snapshot change remains held until that deletion, then uses bound503.
