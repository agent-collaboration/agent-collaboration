# D1 auto mode

Verify a newly created session through the public conversation tools on a declared signed debug Router. The adopted outcomes are `docs/specs/2026-11-13-cli-mcp-sessions/specification.html#R3` through `#R7`, `verification.html#done-d1`, and the D1 r2 plan. Use the current commission for target access and accepted proof corrections.

**Draft: no real-provider qualification yet.** The step driver is `verification/observations/providers/auto-mode/capture.ts`. It drives one declared prompt and captures public state on an already created session. It does not create sessions, answer approvals, infer that an operation completed, or issue a passing D1 verdict. Start/show/options reporting and the full native observation route still need qualification on the declared D1 build. A successful capture is apparatus output only.

## Target and readiness

Use the main skill's launch, signature, configuration guard, readiness and owned cleanup. Keep run plans and evidence under the task's private run directory, outside Cargo target storage. Get the exact build commit, artifact hashes, source/build equivalence, debug root, port, profile, provider executable versions and root allocation before starting. One controller owns the root at a time. A root conversion or another controller's active run must settle first. Never copy an account root.

Run the approved account health route before model input. `codex-router account list --router-root <approved-root>` reports stored OAuth maintenance status; redact all account labels and retain only provider/status/OAuth categories. It cannot prove a credential will work on inference. The runtime's `owned-runtime.ts "$RUNTIME_PLAN" --check` checks readiness and configuration, not credentials. Metadata-only provider inspection must not report logged-out/authentication-required. Stop and return sign-in needs; do not repair credentials.

Claude uses the approved Haiku catalogue value; Codex uses `gpt-6-luna` and low effort through the debug profile. Verify actual model selection before each prompt. Cursor needs an explicit approval for this particular run; omit its live input otherwise and report it unverified. Never substitute a different model to get a passing result.

## Drive and observe

Discover the target Host's current `tools/list` schemas before input. Use the public `conversation_create`, `conversation_prompt`, `provider_session_inspect` or native `session_inspect`, `events_observe`, `approval_list`, and operation read/wait tools. The D1 source declaration determines the added reporting fields. Do not invent a session/start API from the design's future D2 names.

Start two sessions on the same endpoint, using distinct caller-minted UUIDv7 operation IDs. Omit mode entirely for the deciding default case. Do not specify `rootMessageId` or fork: those exercise different scratch semantics. Keep the exact returned targets and any pending operation IDs before further action. A create result or a setting echo is not independent confirmation of the effective mode.

| Row | Action and deciding evidence | Wrong behavior it rejects |
| --- | --- | --- |
| R3 | Read start and show, then the provider's own advertised effective mode. Codex's native settings observation must show workspace-write and reviewer user. Requested mode is auto; any documented fallback has a reason. | Default mode left unchanged; requested value echoed as actual; fallback hidden. |
| R4 scratch | Confirm both existing folders are private and distinct. In session A, create a fresh marker, overwrite with different exact bytes, read those bytes independently, delete it, then run one scratch-only shell command. Each step has an actual completed tool/turn and zero approvals over its complete observation interval. | Inert prompt, missing scratch grant, shell requiring approval, or another session's folder granted. |
| R4 system temp | Separately attempt one harmless fresh marker in system TMPDIR and one in /tmp. Canonicalize first: both must be outside working/scratch roots. Read an approval before any refusal or effect. | Native parent profile still grants system temp; alias or project overlap makes a false negative. |
| R5 | Read the focused admission tests and their run receipts: absent mode, explicit auto, provider-native modes, foreign/bogus modes, and the accepted list. | Planner exists but creation bypasses it; bogus input reaches the provider. |
| R6 | Request a harmless write outside project and scratch. Correlate its real approval, answer allow-once, and independently read exact marker bytes after successful tool/turn settlement. Repeat with fresh markers for deny, cancel and timeout: terminal approval, no effect, no silent refusal before that ending. | An approval exists but cannot release the tool; refusal precedes decision; denial still executes. |
| R7 | Read the endpoint's options in the same run and compare autoRealization with the R6 route actually observed. | Static capability declaration disagrees with effective behavior. |

Before every step, capture the exact target, binding generation, model/mode and observation baseline. Only one prompt is admitted on that session during the step. Correlate operation/input/turn separately; when the public operation has no direct input join, retain the isolated-session attribution limit. Wait on the real operation/tool/turn signal with its bounded wait, never sleep to infer completion. A pending operation is inspected, never replayed.

Retain exact public tool names/arguments/results and scoped metadata events. Do not read provider transcript files or retain model thoughts. Redact auth labels. Inspect recorded action presentation before answering: only the exact harmless marker action may receive an actual offered allow-once option. No persistent grant or borrowed Human identity is allowed. Deny/cancel/timeout use their actual terminal records; a detached waiter is not cancellation. Timeout evidence must come from the injected clock's existing apparatus or a real terminal timeout event, not a sleep or a modified timeout policy.

For the scratch isolation hard cases, include session B's folder, a prefix sibling, `..`, and a symlink escape. A nonexistent marker is inconclusive unless the intended tool attempted the action and its terminal outcome is known. Inspect the permanent fixture assertions too; a list of roots is weaker than actual location enforcement.

Include the interpreter negative: a script stored inside scratch that would write a harmless marker outside project and scratch must not be auto-allowed merely because the script path is inside scratch. Record the actual approval and absence of the outside effect until it is decided. Keep this separate from the positive scratch-only shell command.

## Claude E2 and Codex E3

E2 explicitly requests Claude auto on the approved model, reads what the provider actually selected, and observes a real classifier denial. An auto request that is clamped to acceptEdits is fallback evidence, not proof of classifier escalation. A failed tool_call_update is a notification with no permission responder. An approval synthesized after that notification cannot prove recovery without the real subsequent effect. Until escalation is proven, use the adopted acceptEdits/asksFirst fallback and its ordinary request_permission route for D1 R6. Report the clamped model to Lead; changing models needs its answer.

**E2 uses an independent experiment entry:** Claude's Router mode planner maps `auto` to `acceptEdits` for both creation and later mode changes (`claude_behavior.rs`, `provider_session_setting_update.rs`). Ordinary D1 create/settings calls therefore do not reach provider-native auto. The separately commissioned E2 stdio ACP harness reaches the installed adapter directly while sending model traffic through the allocated debug Router. It adds no product flag or planner bypass. First inspect the selected Haiku model's SDK `supportsAutoMode` capability without a model turn. If it is false or unreported, stop and return that result; another model needs the owner's decision. Only an explicit true flag admits the few authorized classifier/approved-retry/acceptEdits comparison turns. Label any post-allowlist capability projection as such. An `acceptEdits` result on the ordinary Router path is fallback evidence and cannot establish an upstream model clamp. D1's adopted fallback R6 journey remains separate from E2.

E3 uses native workspace-write with approvalsReviewer user. Record exact native settings plus real attempts in scratch, system TMPDIR, /tmp and outside the project. A configured writable-roots list cannot establish the native parent profile's effective grants. If temp cannot be excluded without changing stored settings, stop and return the plan break to Lead.

## Step capture

Read `capture-plan.ts` for the plan. It names an already ready runtime plan, exact target and caller, expected model, a fresh output directory and either snapshot or one prompt. The driver verifies the runtime and source, binds the target to the same service, and refuses Cursor input without the current run's approval reference. Run:

```sh
node verification/observations/providers/auto-mode/capture.ts PLAN.json --check
node verification/observations/providers/auto-mode/capture.ts PLAN.json
```

`--check` is local validation only. The run writes its intent before network input and uses an exclusive output directory so rerunning the same plan cannot repeat a prompt. It captures scoped approvals and event metadata before and after the prompt, plus provider/native mode metadata. `prompt.json` preserves the prompt settlement or pending operation identity without model bodies. A later snapshot plan can collect readback without repeating input.

For the current D1 transport, the provider's actual mode is `settingsCatalog.currentMode` on inspect and `effectiveSettings.mode` on the retained create result. Requested mode and fallback are `requestedMode` and `modeReason`; capture omissions verbatim instead of inventing an actualMode field. Keep the original create result separately for R3. The driver preserves inspect's requested/actual/reason metadata and writes `options-before.json` and `options-after.json` from the exact endpoint's `endpoints_list` channel `autoRealization`. These are declarations to compare with the real R6 route, not independent proof that the declarations are true.

For Claude/Cursor, `event-page-*.json` retains each cursor request and redacted result. The existing observation ledger checks target, epoch, cursor chain and notification agreement; the driver also requires an unchanged binding generation. Result-limited pages are drained before further input, up to ten pages per phase. Resync, oversized events, disconnect, a page that makes no progress or an exhausted bound stop the step. `events-before.json` and `events.json` summarize the baseline and post-baseline coverage without issuing a product verdict. Approval request and tool IDs remain in the redacted event records for ordering checks.

The provider service always returns `continuationGap: true` because attachment is call-local; keep that protocol flag visible. It does not alone invalidate a checked provider cursor chain. A complete cursor chain still needs actual action/turn settlement and marker readback to prove R4/R6. Native observations have no provider cursor; the driver retains their two bounded snapshots and leaves complete native interval coverage unqualified. Both routes remain unqualified on a real D1 target until driven there.

## Evidence and cleanup

Report R3–R7 separately for each agent as supported, unsupported or unknown. Keep actual source/build/root/client identity, command exits, operation/turn/request/tool IDs, ordering, marker readbacks, scoped approval records and test sensitivity receipts. Keep the limits of fixtures, captures and live observations separate. A missing model action, incomplete observer interval or unavailable route is unknown, never a pass based on marker absence.

After all operations settle, close only the sessions this run created. Apply the main skill's exact owned Host cleanup and configuration guard, release the root allocation to its coordinating verifier, and confirm evidence survived. Cursor without per-use approval, unresolved E2/E3 and any unqualified native/reporting path remain explicit in the handback.
