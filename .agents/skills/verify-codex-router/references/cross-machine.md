# Cross-machine

## Current entry

The wrapper retired in 0.1.102. Current execution is `node verification/run.ts "$PRIVATE_DECLARATION"`, with the strict owner-private declaration in `verification/declaration.ts` and retained cases in `verification/case-registry.ts`. The declaration selects two machines, exact signed artifacts, an explicit Tokio/workerd/staging target, governing rows and a deadline. Real state and evidence require the declared canonical cache task roots; current synthetic-only and real-copy boundaries are enforced by the selected entry. Missing mappings remain open and cannot receive confirmation credit. A separate real-copy Mac import uses `verification/import-run.ts` and its own declaration.

## Retained obligations and historical evidence

The sections below preserve the governing obligations and earlier qualification limits. Descriptions of retired wrappers and their inputs are historical; they are not current command contracts. Use the retained source above to determine actual coverage, and report any obligation without a current route as unavailable. This documentation change supplies no new runtime proof or operating authority.


This lasting journey drives reachable OB4 routes through two declared debug
Hosts and retains all ten case results. It is a **partial driver**. Recipient
routes need authenticated exact-model sessions. The driver uses an existing
pre-apply Copy dispatch hold and normal Session retirement/recovery operations;
each still needs qualification on the declared target. Launch, sender acceptance and
component proof do not establish recipient arrival.

Adopted references: board Verification Design OB4/§5c and spec 3 FL-14,
MS-1–MS-12. Decision 1100 (`1f22474d`) separates behavioral retirement replay
from central compact-row component proof. Decision 1110 applies spec 1's
`notWriter` current epoch **if known**: null before a read copy, the retained
epoch after completed authority is acquired. Human recipients, questions and
approvals are outside this journey.

## Run and bind

Use the current TypeScript entry above; the former wrapper’s invocation and declaration are retired.

The retired command is unavailable; use the current entry and declaration above.

The current strict JSON schema is `verification/declaration.ts`. Unknown fields
fail. Keep plans and fresh output directories under an authorized private task
directory outside repositories. This commission forbids `/tmp`. A plan itself
authorizes no build, real enrollment, staging access or deployment.

For a bounded rerun after a driver correction, `recipient_cases` selects named
recipient cases from the table below. Omit it for all cases. Unselected cases
remain blocked in that run; selecting fewer cases cannot establish full coverage.

| Binding | Declaration |
|---|---|
| Controller | Absolute `repository`, fresh `output_directory`, `mode` |
| Two installations | Distinct names, canonical owned `task_directory`, containing `temporary_parent`, selected `port` (0 allows allocation) |
| Native artifacts | `launcher`, `cli`, each with absolute `path`, `sha256`, full `source_head`; signed private copies and their build receipt |
| SSH transfer | Literal SSH host and exact signed Router/CLI artifacts in `verification/declaration.ts`; `verification/machines/remote.ts` owns transfer and launch. Never infer artifacts from mutable `target/debug`. |
| Local central/import | Exact `central_binary` and optional `importer_binary`; importer enables the synthetic unheld case |
| Recipient | Real controlling `caller` SessionRef; `recipient_provider` with adapter/native Claude paths, digests, versions, private `configuration_directory`, optional API-key environment **name**, and `model: claude-haiku-5-5` |
| Session | Matrix requires both already-enrolled endpoints, peer, receiver, relay and machine/access credential environment **names**, never values |
| Central owner HTTP | Matrix requires `central_target.endpoint` and `authentication`: HTTPS `ownerAccess` with `token_environment`, or isolated loopback `localFixture` with a private synthetic assertion file. No implicit login or enrollment |

Build `scripts/verification/cross_machine_native/Cargo.toml` only in the granted
shared window and normal workspace target. Sign, strict-verify, hash and copy the
shared artifacts before returning the window. Keep helper source/lock hashes in
that receipt. Launcher changes require a new receipt; old copies cannot qualify
new authentication wiring. The driver builds nothing.

## Modes and isolation

- `readiness`: launch declared local/SSH Hosts, check hashes/signatures, compare
  operation readiness to publication, call public MCP/CLI, check distinct
  installation identities, and close them. Session configuration is forbidden.
  Omit provider bindings for model-free remote readiness. Exit 0 is readiness
  only, with all behavioral cases not-run.
- `local-conformance`: two local Hosts and the real declared Tokio central.
  Disposable synthetic enrollment uses the existing fixture Access edge;
  registry, forwarding, storage and mail stay real. The edge forwards WebSocket
  bytes unchanged. The per-installation Session gate can pause the declared
  connection and preserves that pause across Host restart. This mode activates
  the reachable behavioral cases; the pre-apply hold uses a separate existing
  central conformance control.
- `matrix`: drive the declared owner HTTP and both real machine WebSocket
  edges using supplied already-enrolled inputs. This performs no deployment,
  login or enrollment. `allow_external_central` is an explicit activation guard,
  and adds no operating authority. Local qualification uses
  the retired local wrapper: it
  owns a real Tokio central and signed synthetic JWKS, enrolls disposable Hosts,
  and invokes this same matrix mode with explicit HTTP/WS declarations.

Remote staging verifies declared local sources before SSH and remote digests
before execution. It writes only the declared task root. Reuse requires a
canonical owner-private root with no live service publication. Different existing
artifact bytes are refused: choose a new hash-named path inside the same root.
The remote agent uses only the standard library. No normal account state is copied.

Sunclaw runtime budget (decision 1112): at least 20 GiB free before transfer,
at most 5 GiB total task footprint, and stop/reap below 15 GiB during a run.
Record space before and after. The agent monitors the running owned Host; these
lightweight limits do not change this Mac's build-heavy storage policy.

Ports 8787, 8788 and 43127 are refused. Production and the standing debug Router
remain untouched. Normal Codex/Router homes stay read-only. The recipient Host
has an absent native Codex backend and launches its declared Claude ACP provider.

## Isolated authentication

Prepare an empty canonical owner-private directory (0700) below the authorized
machine task parent. Reuse that exact directory for both local Host plans and
later G4. The owner runs this in that machine's interactive terminal:

```sh
env CLAUDE_CONFIG_DIR="$PRIVATE_AUTH_DIRECTORY" \
  "$DECLARED_CLAUDE_EXECUTABLE" auth login --claudeai
```

No Router must be running. Copy no credential. Claude keys its macOS Keychain
entry and fallback credential file to this directory, selecting a separate login
from normal Claude authentication. See [official authentication docs](https://code.claude.com/docs/en/authentication).

ACP advertises selectable `haiku`. The owned launch environment binds
`ANTHROPIC_DEFAULT_HAIKU_MODEL=claude-haiku-5-5`; select `haiku`, then inspect model
and authentication before any input. This is the [documented native alias binding](https://code.claude.com/docs/en/model-config), with no different-model fallback.
`loggedOut`, unavailable settings or a different selection blocks input.

## Cases and limits

| Case | Deciding route; remaining qualification |
|---|---|
| Fleet outage | Original pending UUID during pause; stop owner, read closed cache/outbox, restart same installation, resume and compare exact replayed decision |
| Forwarding | Root/reply, CLI/MCP replay, origin/actor, one holder effect, non-post refusal before/after B obtains completed authority, imported unheld refusal, independent offline `lastSeen`; production deadline/cap and central content nonretention unqualified |
| Both directions | Direct messages and board notifications in both actual recipient sessions; event cursor acquired before input |
| Local independence | Actual local notification while that Host's central Session is paused |
| Held copy acknowledgement | Existing pre-apply Copy dispatch hold on the holder installation ServiceId; decode the intended cause Piece, observe the cause outside the acknowledged prefix and unrelated actual mail, then release and observe the notification |
| Distinct handoffs | Public frozen intent IDs, distinct local/remote recipients, stable IDs on replay and correlated arrival; stopped-owner handoff-body readback/reopen unqualified |
| Receipt retention | Pause the actual recipient, freeze original, stop/reopen sender and compare closed-store bytes, replay identical envelope while unsettled, resume and observe arrival; terminal normal retirement and original replay preserve public sender status and cause no second injection in a bounded quiet window. Compact row/reopen/conflict proof remains separate Tokio evidence |
| Offline retry | Pause recipient Session, same-item retries, reconnect, actual output, then no repeated input/output in a bounded quiet window; receipt-retention covers unsettled sender reopen. Provider restart and long-term dedup remain unqualified |
| Automatic board cancellation | Board membership/generation/copy controls remain unqualified across machines; local native and real-Host cases provide their separately declared coverage |
| Wakes | Actual timer arrival, original-operation slot replay, unrelated actual-controller refusal, creator replacement; target replacement and lifetime/timing matrix unqualified |

The central conformance selector is **pre-apply Copy dispatch hold, which
withholds the normal replication acknowledgement** (decision 1118). Address
`/admin/v1/conformance/runtime` at `connection:<holder installation ServiceId>`,
select `copy:<project UUID>`, and use `holdNextDispatch`, `awaitHeldDispatch`,
`resumeDispatchAndHoldNext` for unrelated earlier Copy requests, and
`resumeDispatch` in cleanup. Decode recorded request bytes and require the
intended cause Piece before counting the hold. Mailbox and Registry remain live.
This is not a post-commit acknowledgement-only barrier. Use only the conformance
Worker for this case; qualify its exact bundle/workerd receipt separately.

Normal retention replay reads the exact original item/digest/body from a stopped
owning project store and submits it at `/v1/session`. Never mint a new machine
SessionId when the Host will resume: that intentionally ends its prior session.
Reuse its exact saved SessionId only after Host exit, leave saved notice cursors
unchanged, and close the diagnostic connection before restarting the Host.
Retire only a normal terminal result; unsettled status is a pending observation,
never a fabricated delivered outcome. Model-free replay qualification receives
no actual-recipient or compact-row credit.

Public outgoing project status requires its normal active lead access. When
authorized for the owned fixture, record an owner grant and install the same
controller as its lead before reading. Keep that controller identity throughout.
This grants no receiver `router_show` access: recipient proof remains the typed
input, output and turn correlation below.

Direct messages put a fresh exact marker first, within the Router preview, and
require it in actual recipient AgentMessage output and the ended turn. This
proves the preview path; full-body retrieval needs its own observation.
Notifications correlate the public frozen item ID with the receiver's typed
`TurnStarted.inputId`, require model output after it, and require an ended
`TurnEnded` for that same turn ID. Prepared-push admission preserves this input
identity through the real provider prompt dispatch. Ordinary provider prompts
do not publish a synthetic UserMessage event for this join.
Input echoes, tool-only events, sender acceptance and uncorrelated turns cannot
pass. The bounded provider API always reports `continuationGap: true`; retain
that fact without claiming uninterrupted global continuity. Require an attached
result for the exact target/binding and epoch, reject resync/oversized-event
markers and duplicate/reordered sequences. Snapshot item updates are compacted,
so sequence holes alone are allowed. Negative windows must reach their deadline.
Retain original mutation
operation IDs and inspect before retry; CLI progress followed by an applied error
is preserved as such.

`journey.json`/`journey.log` retain source digests, target bindings, exact calls,
responses, each case and cleanup. Exit 1 means assertion/action/cleanup failure;
exit 2 means partial/blocked coverage. Zero matrix exit requires every case
observed. A failed case does not erase the others. Close recipients, Hosts, edges
and central independently; check recorded Host exits, TCP closure and removed
publication. Read stores only after owner exit and the consumer check passes.
Older readiness/component receipts retain their own pin and cannot establish
final two-machine behavior.
