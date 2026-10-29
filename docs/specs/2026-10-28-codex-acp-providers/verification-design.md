# Verification Design: TypeScript TUI journeys and meaningful regressions

Use the existing **`scripts/verification/tui` TypeScript verifier** to drive the real Codex CLI. Its native adapter owns the PTY; TypeScript owns declared-target qualification, scenarios, readback and original-artifact checks. Supporting API observations confirm provider effect and identity. They do not replace driving the TUI.

This plans proof; no feature journey or product test has run. Inputs: [Requirements](requirements.html), [Specification](specification.html), [Program Design](program-design.html).

## Existing harness and needed extension

The package currently supports **Quota and Sessions**. `capture-run.ts` exposes those surfaces; `public-sessions-run.ts` drives the public Sessions entry. Add an installed-Codex ACP target/scenario in this package. Do not run an existing Quota/Sessions capture and relabel it Codex proof.

| Existing source to reuse | What it establishes | Needed Codex extension |
| --- | --- | --- |
| `source-binding.ts`, `evidence-schema.ts` | Exact source/executable/tool binding; current harness requires Node26.12.1 | Bind installed Codex, candidate Host/router, native adapter, private socket, provider/session/cwd and viewport using the existing artifact/schema patterns |
| `driver-lifecycle.ts`, `public-lifecycle.ts` | Exact driver discovery, bounded launch/settlement, cooperative owned cleanup | Launch unchanged Codex through its real `--remote` session entry; no production replacement/config writes |
| `cua-readback.ts`, `original-artifacts.ts` | Same-child/PTY identity and original capture/readback checks | Actual Codex keyboard scenarios and checkpoints, preserving original PTY/captures and matching native requests |
| `scene-contracts.ts`, `capture-selection.ts` | Explicit scenes and accepted input/selection | Bounded Codex ACP scenario and checkpoint oracle; no generic second capture framework |
| Existing native support in `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test/` | Native capture/PTY ownership and fixed child adapter | Installed Codex launch/input target at this existing adapter boundary; TypeScript creates no second PTY or terminal emulator |
| `verification/observations/acp-metadata-observation.ts` | Public target/session/native API/history readback | Supporting skill/command effect observation correlated with that same TUI child/session; it remains an observer, not the TUI driver |

The extension is **needed**, not an existing option claimed to work. Actual keyboard selection, submitted native input, original artifacts and provider result must refer to the same declared child/session. Receipts alone establish neither visible UI nor a provider effect. Keep existing Quota/Sessions scenarios and rejection tests intact.

## What you will see

### U1 — Both providers' real skills appear

The TS/native-PTY journey opens `$` against Claude and Cursor, sees the expected fixture skill and excludes a command. Late metadata updates without a stalled client; errors are visible beside valid rows. An unqualified original Claude launch shows SourceUnavailable while chat/settings remain usable; it does not count as the required qualified Claude success. **Unverified until run:** new Codex scenes, actual rendering and real provider classification.

### U2 — Keyboard-selected skills actually execute

Select through the real picker, submit distinct arguments and an image, and observe a provider-owned effect absent from the literal prompt. Include leading/nonleading input and multiple distinct Cursor skills. **Unverified until run:** native expansion, path round-trip, preserved content and actual error presentation.

### U3 — Existing slash operations have their distinct effects

Drive `/model`, mode and `/compact` in the same TUI. Read effective settings and Claude's compaction marker plus terminal outcome. Cursor compact shows unsupported and leaves input usable. **Unverified until run:** real command flows, effects and busy/error handling.

### U4 — Conversations and their lifecycle survive

Drive live/reopened chat, interrupt and approval controls for both providers. Read back the admitted single-Text skill message as the exact original `$name` input, including Claude nonleading input that prepares into two Text blocks; keep main's no-synthesized-item behavior for image/multi-block input and separate provider user-group IDs. Refresh skills during an active turn while retaining its stream, identity and terminal result. Keep separate native Codex/reverse ACP controls. **Unverified until run:** actual interleaving, history and owned-process cleanup.

### U5 — Constructor DI supplies the right implementation

Run the real consumer with differing implementations of the same ProviderBehavior trait and observe different literal classified/prepared results. Inspect constructor propagation, dependencies and shipped asset. Force qualified and unqualified Claude launch preparation; verify the exact resulting executable/arguments, preserved environment and immutable constructor input, plus source error and unsupported compact for the unqualified case. **Unverified until implementation:** both session-admission paths, session-state isolation and final normal-build graph.

## Target and lasting journeys

Candidate: clean committed source containing current main; isolated normal signed debug Host on macOS; unchanged installed Codex0.165.0; Claude ACP0.81.0/SDK0.3.280 and qualified native Claude; Cursor2026.10.22-27dcf5f. Requalify updated providers/tools before extending a claim. Use private unused root/socket/port and normal trusted Codex home. Preserve production/config. Exact toolchain/capacity is a delivery-preflight input, not an assumed pass.

- **J1 skills — needed, lasting:** extend `verify-codex-router` with TS-driven actual `$` selection/input and same-child captures/readback for both providers. Supporting native input/list and provider effect distinguish execution from echo. Include empty/late/failed metadata, cwd/session isolation, ambiguity, full-token arguments/images and multiple Cursor selections.
- **J2 commands — needed, lasting:** same TS/native-PTY path drives model/mode/compact controls. Supporting native request, effective setting/typed marker and turn state must agree. Include unsupported Cursor compact and usable idle input after error.
- **J3 preservation — existing ACP metadata journey plus needed TS Codex scenarios, lasting:** drive reopen/interrupt/approval, original selected-skill input history, active-turn refresh and wrapper EOF/signals. Supporting public history keeps genuine prose, user input and metadata distinct. Native Codex/reverse ACP controls stay separate.

## Coverage rows

| Need / obligations | Deciding seam and observation | Passing condition / wrong build rejected | Reuse and remaining unverified |
| --- | --- | --- | --- |
| U1 / R1,C1,V1 | J1: TS harness/native PTY → actual picker, with native list/provider metadata | Correct skills for both providers before skill input, exact ID/scope/cwd, valid rows alongside errors and late invalidation. Empty face, command-title spoof, wrong session or stalled list fails. | Reuse binding/original-artifact/same-PTY controls; Codex scenes and real picker needed. |
| U2 / R2,C2,V2 | J1: actual keyboard selection → native structured input → real Host/FIFO/actor/provider effect | Effect once, exact text/arguments/images; all selected Cursor identities recognized. Echo, explanation, first-only drop, changed identity or lost image fails. Deterministic wire-level peers decide zero-send refusal cases. | Reuse original captures and supporting effect observation; actual expansion/rendering unrun. |
| U3 / R3,C3,V3 | J2: real slash/model TUI flow → native request → marker/settings/terminal | Claude explicit marker+successful terminal and cleared busy; settings readback matches. Unsupported Cursor compact sends no prompt. Ack/title/automatic marker substituted for manual compact or model fallback fails. | Reuse TS lifecycle/capture and extended public observer; actual effects/error presentation unrun. |
| U4 / R4,C4,V4 | J3: same real child/session, live/reopened UI and public history, active-turn refresh/cleanup | Stream/identity/terminal preserved, chat usable, no duplicate uncertain effect or leaked owned child. Wrong PTY, lost history, synthetic discovery prompt or reload disturbing turn fails. | Reuse metadata/live-replay recipe and owned cleanup; actual concurrency/lifecycle unrun. |
| U5 / R5,C5,V5 | Constructor/consumer integration, real TS→Rust metadata and dependency/asset inspection | Host-selected immutable implementation reaches runtime and both admissions; consumer uses only trait; state stays actor-owned; producer values decode; shipped asset matches source. Hidden provider lookup, reverse concrete dependency, shared mutable view or stale asset fails. | Focused automated/structural proof, no extra TUI journey; implementation/normal graph unrun. |

## Tests that can catch the wrong implementation

Take expected values from C1–C3/native schemas or checked producer fixtures, not production helpers. Keep the smallest responsible boundary real.

| Wrong behavior | Valuable regression / independent oracle |
| --- | --- |
| Picker and submitted identity differ | Actual TUI selection and native request match the fixture's identity and provider-owned effect. Screenshot presence alone cannot pass. |
| Wrong behavior injected | Actual consumer receives two scripted trait implementations with different literal outputs; assert emitted value/wire content, not only a constructor or method call count. |
| Claude wrapper is never launched or qualification is ignored | Preparation tests qualify the checked official-bin/pnpm-shim fixture and reject custom/mismatched targets. Inspect actual rewritten launch/import identity; real original-launch integration yields source error, zero skill/compact prompts and working ordinary chat. J1 separately requires qualified real Claude success. |
| Cursor command looks like a skill | Literal producer rows retain distinct terminal command/skill indicators. Cover mimicking titles, exact nested IDs, duplicate/unknown/uninvocable rows and valid rows alongside errors. |
| Skill disappears during a queue wait | Hold dispatch, replace view, release; real ACP peer sees zero prompts and Host receives immediate refusal. No sleeps or timeout increases. |
| Content rewritten incorrectly | Table/property cases preserve independent literal Unicode, whitespace, suffixes and image payloads. J1 separately checks actual multiple-Cursor/single-Claude native expansion. |
| Claude reload loops or publishes after retirement | Controlled Query promises force echoes, descriptor/result changes and retired state/query; actual TS output enters real Rust decoder. J3 separately decides real turn preservation. |
| Compaction succeeds from an ack/title | Real codec/actor cases cover marker+terminal, missing/failed/automatic marker, cancel and uncertainty; only C3's combination succeeds. J2 proves real provider effect. |
| Verifier accepts false evidence | Extend existing rejection tests for wrong source/binary/provider/session/PTY, absent/reordered checkpoint, echo-only output and blank/wrong scene. Assert the real rejection. |

Pure rules use focused tests; DI/serialization/actor ordering uses real-consumer integration; actual interactions use the existing TS/native-PTY journeys. Do not repeat every parsing edge through the TUI. No temporary test files, hand-authored success receipts, terminal emulation or standalone replacement verifier.

If native expansion, Cursor output or active-turn reload contradicts the design, return the failure to Program Design. Do not drop a provider or count unavailable skills as U1 completion. The HTML document-preview limitation is separate from this product verification route; no product journey is reported passed here. Other versions/platforms and release proof remain outside the declared target until qualified.
