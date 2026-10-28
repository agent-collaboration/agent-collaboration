# Mailbox remainder: requirement and proof frontier

Source: 1aea1301292ef847d8f4c277899adb520c9d5875, library9331f5e. Commission: Lead01a15326 (B4 first, then B2c clean move). Verifier B released the scoped build/test/Worker window in01a1532e; workspace-wide commands still need a separate Lead grant. C25 acceptance is bounded native custody plus its exact Worker readmission checkpoint; it is not whole MS/G3 acceptance.

| Requirement | Existing composed evidence | Uncovered central work / owner / constraint |
|---|---|---|
| MS-1 path | Native normal sender -> Registry -> checked Mailbox Submit and physical custody. | Fetch, Received and Report through the recipient's normal session; real recipient inbox is A/G3. Start with those public Mailbox methods. Same-machine bypass remains A's. |
| MS-2 identity | C25 same ID/digest replay and conflict after real reopen. | Preserve exact item and digest after fetch, receipt, terminal report and retirement compaction; no duplicate sequence or reinjection. |
| MS-3 custody/status | C25 commit-before-answer, rollback, postcommit wake loss, unhealthy-owner answer; delivery leaf append/admit and fetch component work. | Never release on fetch alone; Received releases payload, keeps canonical evidence; Held/terminal report order and replay. Router application/complete-prefix ack is A/G3. |
| MS-4 queued/sent | Native accepted Submit is Sent; lost answer retries original item. | Queue/drain and local work isolation are A/G3. Positive Worker MAIL remains unobserved. |
| MS-5 release after replication | Historical project grant check and old holder after move observed. | The board's release gate/frozen handoff is A; whole path G3. Central does not inspect board contents to reimplement it. |
| MS-5a immediate local | No central path required. | A owns the local path and the explicitly named takeover exception; G3 observes it. |
| MS-6 self-contained content | Board typed message/notification envelopes and submitted item/digest. | Fetch exact original content, pointer, sender and recipient; no content lookup or replacement from another line. Context reads are the existing snapshot/board path. |
| MS-7 retention/caps | Canonical custody and permanent row replay; central limits constructor and accounting configuration. | Public Mailbox fetch/receipt/report/retire plus long held/no-expiry retention and before-custody capacity refusal can proceed. Autonomous unfetched validUntil expiry is blocked by owner-identity recovery described below. Physical quota exhaustion is not claimed. |
| MS-8 injection checks | No central injection claim. | A's Inbox/carrier owns cancellation/expiry/supersession checks, coalescing and auto/queue/steer; G3. |
| MS-9 cancels | Historical authority refusal includes cancel. | Public controls-stream fetch/receipt and canonical cancel/report/retirement outcomes, one cancel per original. Board-derived release gate and effective prevention are A/G3. |
| MS-10 wakes | Current BoardDeliveryBody has Message and Notification only. | Remote wake payload/carrier has not landed here; do not invent the kind. Router wake ownership/lifetime rules are outside central's relay policy. Record as carrier/G3 dependency. |
| MS-11 successor continuity | Real old historical MAIL after a move and canonical one-item custody. | Higher-rank successor/replayed lower rank cannot move status target backwards; current target retires inherited identity. Frozen follow handoffs/republication across holder changes remain A/G3. |
| MS-12 deferred kinds | Relay remains generic over BoardDeliveryTypes. | Questions/approvals remain deferred. No speculative variant added. |

## Current pin and routing limits

- Status acknowledgement: library9331f5e DeliveryRequest has StatusFetch but no StatusAck, and StatusStream has no acknowledge method. Existing own-stream fetch can stay; complete-prefix ack awaits C23's adopted API/pin. No raw status-table deletion is added.
- Autonomous mailbox expiry: central physically addresses a mailbox by ServiceId, while public Mailbox::expire_due needs the exact PeerId. The durable peer is in the library's private MailState; MailStorage::state validates it against the supplied owner. ObjectRuntime<MailboxKind> currently receives ServiceId only. On an alarm after reopen there is no request from which to recover the peer, and Mailbox -> Registry is forbidden by the DAG. No public production owner-identity read exists in this pin. Lead routed this as W-C33/C31, corrected in01a15337: `Mailbox::stored_recipient(scope) -> Option<PeerId>` over the singleton state. Once pinned, lookup, construction and expiry share one step-driver transaction and the existing earliest-wake owner. None is not cached. The empty-lookup -> first-admission wake race must be proven. Until then this remains pending; no private SQL, cast, second identity row or per-item timer workaround.
- Own recipient Fetch/Received/Report are unblocked: the admitted session supplies PeerId and its ServiceId, and Session -> Mailbox is an existing edge. The receiver validates the physical source/target; the public Mailbox rule checks the peer.
- Query/Retire/Settle name another mailbox by PeerId. The checked owner backend can be built/proven using a Registry-authorized call as C25 already does. Normal remote routing still needs the committed Registry ServiceId resolution, including a settled absent-mapping outcome. No PeerId/ServiceId reinterpretation is permitted. Keep its normal-path proof separate from the backend proof.

## First permanent product scenarios

1. Submit -> Fetch keeps custody; Held report with no receipt survives beyond history retention and physical reopen with exact item/digest; Received releases payload without regressing Held; terminal report plus retirement compacts but permanently replays identity.
2. A full fixture limit refuses the next item before custody and keeps the first item intact. A retirement releases its accounted slot; any later acceptance still uses its original item identity.
3. Normal native recipient session Fetch/Received/Report returns one correlated answer, with no local section held across the mailbox call. Sender status is canonical; no recipient Inbox/carrier durability is inferred.
4. Controls-stream/cancel and ranked succession are additional public-rule composition cases, not invented central custody algorithms.

Each row remains unverified until its actual owning target runs. Two backend scenarios now have observed pre-fix red at the unsupported checked request (0/2 exit101). The normal recipient scenario has observed pre-fix red after its accepted Submit and missing Fetch route (0/1 exit101, socket reset). An earlier fixture used the outbox item cap instead of the mailbox cap and an earlier normal Submit expectation used the wrong wrapper; both were corrected from the public source and rerun before dependent product implementation. A stale report acknowledges the submitted report sequence while returning canonical newer status/version; its expectation was corrected from the pinned source. Passing proof is not yet recorded for the final edited source. Existing C25 tests remain and must pass after extracting common mailbox failure/health handling; no old proof is silently relabelled to cover new methods.

## Current relay component proof

The final six whole native targets pass35/0/0 with zero filtered: mailbox_lifecycle12,
mail_submit_http3, mail_status4, mail_status_http1, revocation_carryover3,
step_transactions12. Logs: tmp/b1a-build/b4-relay-native-final.log.
The backend tests cover post-compaction reopen replay, a real capacity refusal
followed by retirement and the next accepted sequence, cancellation's reserved
slot and one Controls sequence, original Items payload retained after Controls
receipt, foreign source/peer/installation/CallId guards, permanent settlement
identity/conflict after reopen, ranked succession with public owed-publication
readback and current-target retirement. The last four exact added scenarios had
no pre-fix red; the initial backend/normal missing-handler reds remain recorded.

Scoped native all-target Clippy -D warnings and the portable three-crate Wasm
check pass. The Worker all-target Clippy command exits0 with the same four
known upstream workers-rs template-manifest diagnostics; no Worker runtime or
positive Worker MAIL custody observation is claimed. A3647-file product/build
input guard remained unchanged across these final gates. Workspace check is
separately Lead-authorized in01a15340 and is not inferred from scoped checks.

C31 expiry, C23 StatusAck, normal remote Query/Retire/Settle resolution, remote
wake carrier, and the A/G3 inbox/status application/replication/injection paths
remain pending. This is a central relay component checkpoint, not whole B4 or
MS/G3 completion.
