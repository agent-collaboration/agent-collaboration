# MCP control journeys

An ordinary MCP caller controls a Claude or Cursor Session through the existing typed tools over a declared Host: create, prompt with bounded observation, exact cancellation, caller loss, reattach and close. The caller never speaks ACP. This journey qualifies the first facade deliverable's V23 rows (plan `docs/specs/2026-11-02-acp-shared-control/plans/2026-11-05-mcp-facade-proof-boundaries.md`, VD F). It does not cover the bound-ACP or full-facade obligations.

**Status: draft.** The driver, oracles and sensitivity tests exist and pass under `node --test`.

The driver wire shape and explicit controlled lifecycle profile are qualified locally. A normal controlled Host run passed five cases and stopped inconclusive at load after close. The owning replay repair has a permanent real Host/hub regression; the complete normal six-case journey still needs a fresh declared observation. Actual provider and original CLI profiles remain separately unverified.

Use the peer's explicit `lifecycle` profile for this full controlled journey. Other peer profiles retain their existing offers and cannot substitute for positive close/resume proof. Real providers use the actual capability profiles in VD F; the full controlled case cannot be relabelled to ignore an unsupported method. The journey remains draft until its required targets and observations pass under their separate commissions.

## What it proves and what it does not

| Proves, on the declared target | Does not prove |
| --- | --- |
| Create stores the declared creator and Approver, read back from the owning store | Authenticated identity. Identities are reported, not authenticated |
| One isolated prompt: a baseline, exactly one new accepted, started and ended turn, its reconstructed reply equal to the operation's own reply, a settled operation and one provider dispatch | An operation-to-turn join. The public surface has none, so concurrent streams are not attributed |
| A dropped waiter detaches; the held work completes once, with no redispatch and no implied cancel | Network attachment before the input. The baseline earns reconstructed output only |
| Explicit exact cancellation settles that prompt as cancelled, with one provider cancellation | Every intermediate delta. Snapshots retain the latest item state |
| A stale cancellation gets the typed "not active" refusal and changes nothing; a declared wrong-role actor's setting change gets the typed `wrongActor` refusal and changes nothing | Who requested an operation, after the fact. The operation's requester is not persisted. Prompts have no creator/Approver guard, so the wrong-role case uses the settings gate |
| Resume and load each carry a new correlated reply on the same exact Session | Media or embedded-context input, other config categories, client MCP setup, extensions |

The coverage inventory `verification/observations/mcp-control/acp-inventory.json` maps every member of the locked ACP schema (agent-client-protocol-schema 1.9.1, protocol v1) to its MCP representation or its unsupported, host-owned or compile-gated disposition. A `qualified` row is claimed only when its named case passes on the declared target.

## Declare the target

The plan's schema is `controlPlanSchema` in `verification/observations/mcp-control/control-plan.ts`. Declare:

- **Source and build:** a clean committed source head, the normal build manifest, and the Host and Router hashes.
- **Host:** a debug Host's service directory with its identity and epoch, plus the endpoint and the provider working directory. The MCP URL comes from the Host's `service.json`. The production root `~/.codex-router` and anything inside it are refused; the normal debug root `~/.codex-router-debug` is usable for ordinary checks. Disruptive cases still need their declared owned allocation.
- **Identities:**
  - The actor and Approver are the local operator's existing Human, from `/usr/bin/id -un` (the owner lookup `collaboration-client` uses), or an actual hosted requester acting as itself.
  - Never borrow a Session to pass the schema.
  - The wrong-role requester is a real identity that is neither creator nor Approver.
- **Peer:** `controlled` names the peer's receipt and its release sentinel; `provider` means a real Claude or Cursor, which keeps no receipt.
- **Run limits:** the cases, the prompt and hold texts, the effect budget (Sessions and prompts per case are fixed in source), an immutable deadline, and a fresh evidence directory.

**Controlled peer.** The normal-dependency `automation-debug-host` launches it through its existing Cursor selectors:
- `--cursor-acp-executable <node>`;
- `--cursor-acp-arguments` set to `crates/acp-client-runtime/tests/support/behavior-agent.ts <receipt> lifecycle cursor_login <release-sentinel>`.

Text `hold` holds the turn until the sentinel appears, which completes it as `HOLDINGafter`/end_turn, or until an exact cancellation. The receipt records each `session/prompt`, `session/cancel` and `{kind:"release"}` row. The driver writes the sentinel only to release its own held prompt, retires that marker once the release is confirmed, and refuses to start another hold while any marker remains.

## Drive

From the repository root, with the verification package's Node:

```sh
node verification/observations/mcp-control/run.ts PLAN.json --check
node verification/observations/mcp-control/run.ts PLAN.json
```

`--check` reads files and runs only `git` and the owner lookup. It opens no socket and calls no tool. It checks:
- the source head and a clean tree;
- the Host and Router hashes against the manifest;
- the service identity and epoch;
- the Human against the owner lookup;
- the effect budget, the deadline and a fresh evidence directory;
- that the release sentinel is absent;
- the inventory: the `Cargo.lock` pin, and the locked names re-derived from the crate's registry source.

The run sends only ordinary MCP: `initialize`, `tools/list` (checked against the inventory), then the declared cases. Each case uses its own fresh Session.

| Case | Deciding observation |
| --- | --- |
| `lifecycle` | Create, stored roles, baseline, one attached prompt, continuation reads to the turn's end, operation readback, peer receipt, then close |
| `callerLoss` | The prompt call is aborted after HOLDING is observed. The outcome must be `callerAborted` and the operation not yet terminal. Then the sentinel releases the work, and the operation wait reply, one prompt, zero cancels and one release follow |
| `explicitCancel` | No release marker may remain. `conversation_cancel` for the held prompt's exact operation, naming the binding generation the latest observation reported (the backend refuses any other); the cancel settles, and the prompt settles `cancelled` with one peer cancellation |
| `staleCancel` | After a settled prompt, a cancellation aimed at it, with the real generation, must get `notFound`/`validation`/effect `none` for its own operation and target, change nothing and reach no provider |
| `wrongRole` | `conversation_settings_accept` from the declared wrong-role actor must get `wrongActor` at the exact target, with effective settings and stored roles unchanged and no peer prompt |
| `loadResume` | Close, `conversation_resume`, a new reply; close, `conversation_load`, a new reply. A profile refusal is inconclusive, not a pass |

Close, resume and load answer with an admission that may still be open. The driver follows its own operation ID through show, and one bounded wait if needed, to a terminal, applied, confirmed record on the exact target. HTTP, protocol, schema or unrelated refusals never satisfy a negative case. Operation IDs are canonical UUIDv7, the only form the carrier tools accept. The first case that does not pass stops all later mutating input; its IDs and known targets stay in the evidence. The process exits 0 only when every case passes and the store holds no non-terminal operation.

## Evidence

Under the plan's evidence directory:
- `exchanges.jsonl` holds every call's exact arguments and raw outcome, including streamed `notifications/codexRouter/observationEvent` frames in arrival order;
- `verdicts.json` holds each case's verdict and the store's non-terminal count.

Integers stay exact. A verdict can be rechecked from these files alone.

## Oracle rules

- Bounded reads continue only from the last observed `{epoch, afterSequence}`. A replayed or fabricated sequence fails.
- A stale epoch's resync is inconclusive, never complete output. `continuationGap: true` alone is not loss.
- Each notification must equal its result's event and carry that event's own cursor.
- A turn ends in `turnEnded.outcome` exactly as the producer serializes it: `{kind: "ended", stop_reason, local_cause}` or `{kind: "lost", reason}`. The field names are snake_case: serde renames only the variant tag (session_event.rs:141-151). A camelCase form is refused.
- Stop-reason values are compared across their vocabularies. The turn event and the prompt settlement use camelCase values; the operation snapshot and wait settlement use snake_case values; an unknown reason is `{unknown: text}` in each and never matches a known one.
- A lost turn fails. A turn ended for a Router-local cause is inconclusive.
- A direct prompt is accepted at its `turnStarted`, which carries its own input ID. `inputAccepted` is published only for a steer injected into an active turn (provider_session_actor.rs:398-407).
- On the isolated Session, an injected `inputAccepted` event, an extra turn or an extra operation makes the case inconclusive.
- A create answer must name the operation that was submitted and a Session on the plan's own service and endpoint before anything else uses it. A self-consistently wrong answer stops the case with no further input. The Session ID is provider-generated and taken as returned. Once a create answer passes that check, its Session stays named in the case verdict even when a later step fails.
- A positive inference case (stop reason endTurn) needs an actual agent-message reply with non-whitespace text, beyond cross-view equality. A cancelled outcome may end with no output.

Sensitivity is permanent in the colocated tests:
- wrong target, including the same raw ID on another endpoint;
- the recorded mcf11 direct-prompt shape, an invented camelCase outcome and a turn start with an empty ID;
- a lost turn, an unknown stop reason and a Router-local cause;
- an injected input on the isolated Session;
- a validated target kept after a later store or observation failure, and never kept after a wrong-target answer;
- false completion;
- lost or truncated reply;
- duplicate dispatch;
- fabricated or skipped cursor;
- notification disagreement;
- stale resync;
- unsettled or foreign operation;
- release before prompt;
- each `--check` refusal.

## Cleanup

Each case closes its Session through `conversation_close`, with the declared creator as requester. That Session's close is performable, so the existing close rule ([ACP metadata](acp-metadata.md)) is satisfied. The allocation's owner stops the Host by its recorded identity. Unknown outcomes keep their operation IDs and are never retried.
