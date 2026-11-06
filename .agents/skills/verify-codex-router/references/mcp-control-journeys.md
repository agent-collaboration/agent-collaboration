# MCP control journeys

An ordinary MCP caller controls a Claude or Cursor Session through the existing typed tools over a declared Host: create, prompt with bounded observation, exact cancellation, caller loss, reattach and close. The caller never speaks ACP. This journey qualifies the first facade deliverable's V23 rows (plan `docs/specs/2026-11-02-acp-shared-control/plans/2026-11-05-mcp-facade-proof-boundaries.md`, VD F). It does not cover the bound-ACP or full-facade obligations.

**Status: draft.** The driver, oracles and sensitivity tests exist and pass under `node --test`.

The driver wire shape and explicit controlled lifecycle profile are qualified locally. After the owning load-after-close repair, the normal controlled Host run mcf13 passed all six cases at source 581eecbec. The strengthened history assertions and new two-lifetime continuation profile below have only oracle tests so far; their affected real-path observations remain open. Actual provider and original CLI profiles remain separately unverified.

Use the peer's explicit `lifecycle` profile for this full controlled journey. Other peer profiles retain their existing offers and cannot substitute for positive close/resume proof. Real providers use the actual capability profiles in VD F; the full controlled case cannot be relabelled to ignore an unsupported method. The journey remains draft until its required targets and observations pass under their separate commissions.

## What it proves and what it does not

| Proves, on the declared target                                                                                                                                                                                                                                  | Does not prove                                                                                                                                                                           |
| --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Create stores the declared creator and Approver, read back from the owning store                                                                                                                                                                                | Authenticated identity. Identities are reported, not authenticated                                                                                                                       |
| One isolated prompt: a baseline, exactly one new started and ended turn with no other accepted input, its reconstructed reply equal to the operation's own reply, a settled operation and, on the controlled peer, exactly one provider dispatch in its receipt | An operation-to-turn join. The public surface has none, so concurrent streams are not attributed. A real provider's dispatch count: it keeps no receipt, so verdicts name it unavailable |
| A dropped waiter detaches; the held work completes once, with no redispatch and no implied cancel                                                                                                                                                               | Network attachment before the input. The baseline earns reconstructed output only                                                                                                        |
| Explicit exact cancellation settles that prompt as cancelled, with one provider cancellation                                                                                                                                                                    | Every intermediate delta. Snapshots retain the latest item state                                                                                                                         |
| A stale cancellation gets the typed "not active" refusal and changes nothing; a declared wrong-role actor's setting change gets the typed `wrongActor` refusal and changes nothing                                                                              | Who requested an operation, after the fact. The operation's requester is not persisted. Prompts have no creator/Approver guard, so the wrong-role case uses the settings gate            |
| Resume and load each carry a new correlated reply on the same exact Session                                                                                                                                                                                     | Media or embedded-context input, other config categories, client MCP setup, extensions                                                                                                   |

The coverage inventory `verification/observations/mcp-control/inventory/acp-inventory.json` maps every member of the locked ACP schema (agent-client-protocol-schema 1.9.1, protocol v1) to its MCP representation or its unsupported, host-owned or compile-gated disposition. A `qualified` row is claimed only when its named case passes on the declared target.

## Declare the target

The plan's schema is `controlPlanSchema` in `verification/observations/mcp-control/admission/control-plan.ts`. Each folder's responsibility is in `verification/observations/mcp-control/README.md`. Declare:

- **Source and build:** a clean committed source head, the normal build manifest (no `test_guard_features`), and the Host and Router artifacts. Both must be that manifest's own entries at their recorded paths and hashes.
- **Host artifact:** select it explicitly with `host.kind`. `automationDebugHost` is the manifest's `automation-debug-host`; `foregroundRouterHost` is the Router's own foreground Host, the manifest's `codex-router` and the declared Router itself. The declared Host PID must be live and running that artifact (`/proc/<pid>/exe` on Linux, the first `lsof -d txt` path on macOS).
- **Host context:** `hostContext` names the automation Host's own `debug-host-context.json` (kind `debugHostPrepared`). It is required for a continuation. Launch this proof profile with explicit `--router-root <canonical-root>` and `--collaboration-directory <canonical-root>/agent-communication`. The Host also permits a run-directory default when a Router root is supplied; this journey does not select that layout. The marker must be in its run directory, with workspace `<runDirectory>/agent-workspace` as the plan's working directory and the selected collaboration directory as the plan's service directory. Its `port` must be the Router proxy port the running Host published in `service.json` `routerProxyEndpoint`; the MCP URL's port is a different one and never satisfies it. Its PID must be the plan's Host PID. Its `cursorProvider`/`claudeProvider` is the provider launch, bound with the executable's digest and each file argument's.
- **Host:** a debug Host's service directory with its identity and epoch, plus the endpoint and the provider working directory. The MCP URL comes from the Host's `service.json`. The production root `~/.codex-router` and anything inside it are refused; the normal debug root `~/.codex-router-debug` is usable for ordinary checks. Disruptive cases still need their declared owned allocation.
- **Identities:**
  - The actor and Approver are the local operator's existing Human, from `/usr/bin/id -un` (the owner lookup `collaboration-client` uses), or an actual hosted requester acting as itself.
  - Never borrow a Session to pass the schema.
  - The wrong-role requester is a real identity that is neither creator nor Approver.
- **Peer:** `controlled` names the peer's receipt and its release sentinel; `provider` means a real Claude or Cursor, which keeps no receipt. A real provider's qualified verdicts carry `dispatchWitness: "unavailable"`: the isolated operation, input, turn, nonblank reply, readback and no other admitted work still decide the case, but the provider-side dispatch count is unobserved, never a passed check. `callerLoss` and `explicitCancel` need the controlled peer's actual causal hold and are refused for a real provider.
- **Run limits:** the cases, the prompt and hold texts, the effect budget (Sessions and prompts per case are fixed in source), an immutable deadline, and a fresh evidence directory.
- **Continuation (two-lifetime profile only):** `continuation` is `null` for every other profile. `{phase: "retain", record}` runs `continuationRetain` as the only case and writes the record exclusively inside this lifetime's evidence. `{phase: "reload", record, recordSha256}` runs `continuationReload` as the only case against the kept record, bound by its digest and kept outside the new evidence.

**Real-provider model.** A real peer must declare `peer: {kind: "provider", model: "claude-haiku-5-5"}` for the approved Claude run. Missing or empty `peer.model` is refused during no-effect plan parsing. The driver sends it as `conversation_create.model` and refuses `createdWithoutSettings` or nonempty `notApplied` before any prompt. Immediately before every real-provider prompt, including after resume and load, ordinary `provider_session_inspect` must name the exact target and expose one model option at `settingsCatalog.configOptions` with `category: "model"` and `currentValue: {kind: "select", value: <declared model>}`. Each read is recorded in `exchanges.jsonl`; missing, ambiguous or different model stops before input. The driver never reapplies or falls back after drift. Resume/load restore their returned settings catalog and do not carry a model setting, so the creation choice alone cannot qualify later input. Controlled-peer plans keep their existing shape and do not acquire model checks.

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

`--check` reads files and runs only read-only local commands: `git`, the owner lookup and, on macOS, `lsof` for the declared Host process. It opens no socket, calls no tool and signals nothing. It checks:

- the source head and a clean tree;
- the Host and Router as the normal manifest's own artifacts, the selected Host kind, and the live Host process running the Host artifact;
- for a continuation: its lone case, the automation Host and its context, and the provider launch; for reload, the kept record's digest and continuity: the same service, endpoint, Session, working directory, Host and Router build, prepared context (run, service and workspace directories, profile, native model, Router proxy port, Router root and peer registry), provider launch and caller, and a new service epoch. Only the Host PID and service epoch may change;
- the service identity and epoch;
- the Human against the owner lookup;
- the effect budget, the deadline and a fresh evidence directory;
- that the release sentinel is absent;
- the inventory: the `Cargo.lock` pin, and the locked names re-derived from the crate's registry source.

The run sends only ordinary MCP: `initialize`, `tools/list` (checked against the inventory), then the declared cases. Each case creates its own Session except `continuationReload`, which must load the exact Session retained by the first lifetime.

| Case                 | Deciding observation                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `lifecycle`          | Create, stored roles, baseline, one attached prompt, continuation reads to the turn's end, operation readback, peer receipt, then close                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| `callerLoss`         | The prompt call is aborted after HOLDING is observed. The outcome must be `callerAborted` and the operation not yet terminal. Then the sentinel releases the work, and the operation wait reply, one prompt, zero cancels and one release follow                                                                                                                                                                                                                                                                                                                                                                                                 |
| `explicitCancel`     | No release marker may remain. `conversation_cancel` for the held prompt's exact operation, naming the binding generation the latest observation reported (the backend refuses any other); the cancel settles, and the prompt settles `cancelled` with one peer cancellation                                                                                                                                                                                                                                                                                                                                                                      |
| `staleCancel`        | After a settled prompt, a cancellation aimed at it, with the real generation, must get `notFound`/`validation`/effect `none` for its own operation and target, change nothing and reach no provider                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `wrongRole`          | `conversation_settings_accept` from the declared wrong-role actor must get `wrongActor` at the exact target, with effective settings and stored roles unchanged and no peer prompt                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `loadResume`         | Close and `conversation_resume`: the operation wait's settlement is `resumed` with history `historyUnavailable`, and the new epoch's baseline holds no turn. Then a new reply. Close and `conversation_load`: the settlement is `loaded`, the baseline is a new replay epoch holding exactly the earlier exchanges in order as historical turns ending `{unknown: "replayed"}`, read whole across result-limited reads (at most 10), then a new reply. Then close. A profile refusal or an unretained settlement is inconclusive, not a pass                                                                                                     |
| `continuationRetain` | Lifetime one of the two-lifetime profile: create, one isolated prompt, then `conversation_resume` and `conversation_close` each refused as `unsupportedCapability`/`binding`/`none` on its own operation and target with no owning-store record; then the record of target, service epoch, working directory, Host, Router, provider launch, caller, stored roles, history and operation IDs                                                                                                                                                                                                                                                     |
| `continuationReload` | Lifetime two, after the owned controller has settled lifetime one and reopened its context with `--resume-run-directory`. Before any effect: the record's digest and continuity, the same stored roles, `provider_session_inspect` naming the exact kept Session as `unloaded`, and the live `endpoints_list` in the new service epoch with that exact endpoint available on an actual external-provider binding. Then a real `conversation_load` settled `loaded` and bound to that same live binding (binding ID and generation) in the new service epoch, the recorded history replayed in order, and one new isolated prompt. No new Session |

| Profile              | Cases                                                                                 | Effects                                 |
| -------------------- | ------------------------------------------------------------------------------------- | --------------------------------------- |
| Full controlled peer | `lifecycle`, `callerLoss`, `explicitCancel`, `staleCancel`, `wrongRole`, `loadResume` | 6 Sessions, 7 peer prompts              |
| Claude full reattach | `loadResume`                                                                          | 1 Session, 3 prompts, no hold or cancel |
| Cursor retain        | `continuationRetain`                                                                  | 1 Session, 1 prompt, 2 refused controls |
| Cursor reload        | `continuationReload`                                                                  | 0 new Sessions, 1 load, 1 prompt        |

Each attached prompt submits the declared harmless prompt marked with its case step, `<promptText> [<step>]`, for example `[reattach-first]`, `[retain]` or `[reload]`. Replayed history is compared in order against that exact submitted text and the observed reply, so a reordered or foreign replay fails even when replies repeat. The continuation record keeps the exact text.

The owned controller, never this driver, starts and stops each Host lifetime; each lifetime has its own plan, fresh evidence and immutable deadline. The original CLI turn is a separate observation on the pinned unforked Codex 0.165.0.

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
- reattach history: missing, canned, reordered, foreign or live-looking load history; history after resume or a resume settlement reporting it; another settlement kind; a stale post-load terminal or injected input; a replay baseline that never completes;
- continuation: a supported resume or close, or a refused control with a store record, in lifetime one; in lifetime two an already attached Session, an inspect for another Session, a reused epoch, a changed launch, a record digest mismatch, a fresh provider Session, a changed stored creator, a changed profile, native model or peer registry, a port that is not the Router proxy, a live inventory from another epoch, an unavailable endpoint, no provider binding, a load bound to a stale epoch or another binding, and canned or missing history after load;
- prepared context: a marker outside its run directory, another workspace or service directory, a non-canonical service directory, no or another published Router proxy port, and another Host PID;
- target: a Router or Host that is not the normal manifest's artifact, test guard features, a Host PID not running the Host artifact, a foreground Host that is not the Router, and held cases on a real provider;
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

Full-profile cases close their Session through `conversation_close`, with the declared creator as requester. The readback must name `conversationClose`, and operation wait must retain a `closed` settlement on that exact operation and target. An acknowledgement or unavailable output cannot qualify close, and a Closed settlement does not prove process cessation. Unsupported-method profiles retain their typed refusal instead. The existing close rule ([ACP metadata](acp-metadata.md)) still applies. The allocation's owner stops the Host by its recorded identity. Unknown outcomes keep their operation IDs and are never retried.
