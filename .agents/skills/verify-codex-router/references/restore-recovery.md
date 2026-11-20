# Restore recovery

An optional mode setter may fail or remain silent without keeping a loaded session unusable. A load that never answers ends locally, and a deliberate retry of the same session can proceed. A definitely lost session gets a new-session nudge; the Router never replaces it automatically.

**Status: partially qualified at `8cb0ec550`, not delivery acceptance.** The affected scripted selection and three current sensitivity witnesses passed their required green/red/restored sequence. Five Cursor no-tools turns passed across one owning Host reopen; Claude's first Haiku request was refused by a failed fast-off setting and its remaining live cases await a fix head. Cursor did not advertise resume. The receipt index is `tmp/d1/verifier/followup-8cb0ec550/report.md`; do not carry its observations to a new head without checking the delta. The existing MCP driver covers normal reattach, not controlled restore faults. Missing executable cases remain setup gaps.

Inputs: adopted outcome and corrections, exact source/build/signatures, permanent case/seed mapping, injected deadlines, provider/client configuration, isolated evidence home and, for live runs, a fresh debug-root allocation and input budget. Return three separate recovery verdicts, saved-setup and kept-protection evidence, each unexercised branch and exact owned cleanup. Acceptance remains with the commissioning Lead.

## Contract and proof boundary

The adopted deliverable (a) uses the session-control-delegates design dated 2026-11-19: Verification U14/U16, Specification R30/R31/R32/R35/R40, and design-review F1/F2/F8. Obtain those actual sources from the commission. Apply the commission's corrections over older hold assertions:

- No restore mode mismatch blocks prompts. Independent explicit non-mode uncertainty and fast behavior keep their existing contracts.
- The optional setter wait is 30 seconds on the one injected clock. A subsequent same-ID restore skips its optional setter while an earlier setter is pending.
- A local timeout cannot revoke a sent ACP setter: the protocol carries no run generation. Prove discarded late replies, no stale local observations or saved writes, and no duplicate input. Remote application of that sent request is a known boundary, not a failed local-exclusion check.
- Only a confirmed explicit **mode** set rewrites the saved mode. Roots and scratch are written at create. Restore, fallback, refusal and no-wire changes preserve all three stored setup fields exactly.
- The new summary/deltas contract belongs to deliverable (b). For (a), read the delivered operation payload and inspect provenance; do not claim the deferred report is proven.

Scripted proof keeps the real client/connection, admission/activation and operation/store boundaries relevant to its assertion. It controls only the external peer and the injected clock. Live proof checks what the actual vendor produces; it cannot manufacture every refusal, contradiction or silent reply. Report the two layers separately.

## Bind the executable route before driving

The permanent source homes to inspect on the declared candidate are:

- [ACP restore](../../../../crates/acp-client-runtime/src/agent_session_client/provider_session_restore.rs): load/resume, history replay and optional mode application feed pending admission.
- [Connection owner](../../../../crates/acp-client-runtime/src/agent_session_client/provider_connection_task.rs): current-attempt registration, routing, grant and actor activation.
- [Setting application](../../../../crates/acp-client-runtime/src/provider_setting_application.rs): wire response correlation and injected response wait.
- [Race scenarios](../../../../crates/acp-client-runtime/tests/integration/simulated_races/superseded_restore.rs) and [scripted peer](../../../../crates/acp-client-runtime/tests/integration/simulated_races/scripted_line_agent.rs): causal receive/release barriers. An old test demanding LocalBusy after the deadline is superseded.
- [Mode fixture](../../../../verification/fixtures/permission-mode-agent.ts): provider catalogs, refusals, restore setup and fast state. Check the candidate's actual load/resume fault controls rather than assuming this fixture can supply them.
- [Saved setup](../../../../crates/collaboration-runtime/src/provider_session_record.rs) and [explicit settings](../../../../crates/codex-router-host/src/external_provider_supervisor/provider_settings_control.rs): owning E2 read/write and explicit-mode settlement.
- [Tool ownership reducer](../../../../crates/provider-session-machines/src/tool_ownership.rs): observed IDs remain known for the connection lifetime. Only never-seen IDs can use unknown-ID fallback. Loading replay may refresh ownership. A retired update publishes only for its same generation, LiveIdle phase and an existing projected item; this never permits a new announcement or callback.

Require permanent executable cases mapped to the observations below, exact selected-case discovery and no zero-match success. Per-request/session holds must let B and an independent session progress while A stays held. An event sink that discards every event cannot decide stale replay or activation. A method-name list without target/input identity cannot establish exact dispatch count. Return those fixture gaps before running dependent proof.

Normal public reattach uses the existing script described by [MCP control](mcp-control-journeys.md) and its [README](../../../../verification/observations/mcp-control/README.md):

```sh
node verification/observations/mcp-control/run.ts PLAN.json --check
node verification/observations/mcp-control/run.ts PLAN.json
```

Here `PLAN.json` is the actual admitted run plan, built against `controlPlanSchema`; select only its declared cases. A normal lifecycle pass cannot be relabelled setter-timeout, silent-load or lost-session proof. This journey adds no script or fault-control flags. If the declared cases cannot be driven by permanent tests or the assigned apparatus, record the absent route and return it to Lead.

## Scripted recovery witnesses

Use both load and advertised resume paths, and Claude-like and Cursor-like behavior where their mode contracts differ. Register waits on the one injected clock before advancing it; drive request-received, reply-released and dispatch barriers. Do not sleep, advance Tokio independently or wait for shutdown to manufacture a product ending.

| Journey | Drive | Passes only when | Wrong build it must catch |
| --- | --- | --- | --- |
| Setter timeout → reload | Restore A succeeds at the peer; its offered mode setter stays unanswered. Reach the registered 30-second deadline; submit one input. Close A, deliberately reload the same ID as B while A's setter is still pending, then submit B's new input. Release A's late reply after B progresses. | A and B activate and each new input dispatches once; B sends no overlapping optional setter. A's late reply cannot publish stale local mode/catalog/grant or overwrite saved setup; B's routing remains valid. Actual mode is unconfirmed where no surviving readback establishes it. | Keep a pending-set fence blocking B; send another optional setter; publish A's stale reply into B; duplicate an input. |
| Load timeout → end/retry | Keep A's load/resume request unanswered on a live connection. While it waits, complete input on another session. Advance the adopted load deadline; request a deliberate same-ID retry B and release A/B separately, including A after B. | A has a terminal local no-answer outcome without shutdown; remote effect remains unconfirmed if sent. A's routing/replay/queued input are retired. B completes, and A's late reply/replay cannot activate A, erase B or redispatch old input. | Bound only on shutdown; strand same ID; keep expired activation/replay alive; stale completion forgets B routing. |
| Lost session → nudge | Peer returns resource-not-found. Separately return load refusal and generic failure; keep these classifications distinct. Complete another session's input. | Each ends locally with its own plain sentence/next step. Definitely lost recommends starting a new session with the saved setup; no automatic replacement, retry or failed-attempt input is emitted. | Bare code; classify every error as lost; auto-create a replacement; silently replay failed input. |

The declared load deadline is required; do not invent its duration from the setter's 30-second wait. Capture operation/attempt/generation/input IDs and actual response stage/effect/reconciliation. A test harness timeout merely detects a stuck candidate; it is not the product's terminal no-answer result. Test both old reply before B's completion and old reply after B, while preserving B's ability to make progress.

Retain the offered, unoffered, refused, silent, valid-other and malformed/unconfirmed setter variants. Every usable variant needs one real agent-side dispatch, honest actual/unconfirmed provenance and unchanged E2. Add a positive control: confirmed explicit mode change, owning store reopen and restore requesting the new saved mode. This rejects an implementation that disables all writes to pass negative comparisons. Compare the raw stored setup fields, including null versus empty roots; do not require the entire SQLite file or unrelated operation timestamps to stay identical.

## Keep the protections separate from hold removal

Inventory the candidate's deletion/reference diff and its transition/action tables. Rank/mismatch state/refusal and special no-wire repair paths must be absent from executable production/test teaching; historical evidence is not executable code. Compilation alone cannot decide removal.

For each kept behavior, read the permanent witness and its intended wrong-build assertion: current-generation auto grant versus named/unconfirmed, fast-off reapply, legacy/null setup, general non-mode SettingsUnresolved, close wins, production-call cancellation before the turn, post-apply settlement after caller detach, immutable old tool ID after a late tool_call_update, per-session mode-write order and stale-restore/routing exclusion. Preserve cross-session progress and cancellation cleanup as well as same-session ordering.

Use reducer/property coverage and causal race scenarios alongside the real Host/store boundary. The Implementer supplies product cases/fixtures; Verifier checks reach and sensitivity. For each required seed, preserve baseline source/hash, successful compile/discovery, the actual intended failure, exact restoration and restored-green receipt. A generic setup error or an obsolete patch is not sensitivity proof. Honor the current freeze and mutation-owner agreement.

## Reachable live Claude and Cursor observations

The four separate 11a [session-control runs](session-control-journeys.md) have their own private-root target and approved selective ACP gate. This section describes the existing restore-recovery matrix.

Launch and readiness use the main skill's declared signed Router/owning Host, readonly normal Codex/profile guard and exact process ownership. Run only on allocated `~/.codex-router-debug`. Obtain a fresh allocation after the declaration; do not copy an account root, change auth/global settings, replace production or borrow old provider identities. Provider health/eligibility is setup evidence. Authentication failure is an environment block, not a recovery verdict.

Budget: at most six no-tools inputs for Claude on **Haiku**, and six for Cursor on the **approved advertised Grok** value. Pass `model: "haiku"` to Claude creation; never allow a Fable default to reach an input. Independently inspect the selected model before every input, including after restore. No model substitution or extra retries. Preserve companion settings. Use actual returned SessionRefs and operation IDs, UUIDv7 for identifiers minted by the caller, and the current published tool schemas.

1. On Host A, create three fresh cases per provider: auto, named (`default` for Claude, `ask` for Cursor), and a separate named session whose mode is explicitly confirmed as `plan`. Use distinguishable scratch/work-root setup. Save setup-only wire and E2 fields, then one independently model-qualified no-tools READY primer per case: three inputs per provider.
2. Stop only the owned Host and reopen the same root once within the allocation. Load the auto and explicit-plan identities; use advertised resume for the named identity. Inspect actual mode/provenance and reselect the approved model before one READY per restored case: up to three final inputs per provider. Unsupported resume stays NOT RUN; do not substitute a load.
3. Compare raw E2 fields before and after each exercised restore. Confirm the explicit saved mode survives the owning reopen and roots/scratch stay unchanged. Compare actual setup wire only where the qualified filtered capture observes it; preserve absent directories/meta as absent rather than inventing them.

`createdWithoutSettings` still creates an identity: retain it and the failed setting, inspect it, and never repeat creation. If an independent fast/non-mode hold refuses the primer, retain that refusal and return the remaining dependent cases to Lead. Do not clear it with Accept to manufacture restore usability. A pending/uncertain operation is inspected or waited on by its existing ID; never replay its effect.

For every input, keep the independent baseline, one accepted input/turn and terminal operation/turn. Repeated READY text alone is not proof of exact dispatch count. At the qualified pin, completed public operation snapshots omitted `inputId`: retain isolated-turn attribution and name the absent public/vendor dispatch join. Do not assume a snapshot's `operation` field wraps another object; it names the operation kind. No tool execution or approval decision is part of this budget. Those no-tools turns show usability, not real grant enforcement.

If resume is unsupported, an expected catalog/model is absent or a session did not persist, retain that exact result and mark the affected case NOT RUN/unknown; do not relabel load as resume or create unbudgeted replacements. Capture naturally observed differences and refusals using today's payloads and inspect. Vendor silence, deliberate contradictory responses and deterministic lost-session codes remain scripted coverage unless separately authorized and reachable. Never alter provider transcripts, credentials, catalog or network to force them. This journey commissions no Codex/Luna live input.

## Evidence, cleanup and return

Keep the declared SHA/version, signed executable hashes, fixture/provider version, injected deadlines, exact target/operation identity, selected case counts, raw failures, seed/restored results, E2 before/after/reopen and live source of actual mode. Capture only safe setup/operation metadata before writing: no thoughts, provider transcripts, credential values or unrelated session bodies. Preserve the main skill's configuration guard and ID ledger.

After each step settle the exact operation rather than replaying an uncertain effect. At the end, close only owned sessions, retain terminal/applied/confirmed readbacks, and stop only recorded owned processes. Require lifecycle/configuration checks, no owned children/root lock, and evidence survival; then return the allocation. An unexpected runtime exit or expired allocation leaves incomplete cases unknown.

Return setter-recovery, load-recovery and lost-nudge results separately, each with target, entry, observation, wrong-build sensitivity and limits. Also return unchanged E2 negatives, confirmed explicit-mode persistence, kept-fix coverage and cleanup. Until the declared-target cases and reachable live pass run, report this recipe as draft with those exact gaps; neither source inspection nor apparatus validation is a product pass.
