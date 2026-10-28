# Verification Design: prove Claude and Cursor through Codex CLI

This document plans proof. It records no feature execution. Inputs: [Requirements](requirements.html), [Specification](specification.html), [Program Design](program-design.html).

## What you will see

### U1 — Both providers' real skills appear

Open `$` in unchanged Codex CLI against Claude, then against Cursor. See the correct fixture skill for that provider and cwd, before invoking a skill. Check a late inventory and a failed source. **Unverified until run:** first-session provider classification, exact native IDs/ambiguity and native error display.

### U2 — The selected provider skill runs

Select the fixture through the actual picker, attach distinct text and an image, and observe a provider-owned effect absent from the literal input. Check leading/nonleading use, multiple distinct Cursor selections, and refused invalid forms. **Unverified until run:** Claude final-Text expansion, Cursor native ID recognition, reference round-trip and zero-send refusals on actual wiring.

### U3 — Existing commands have their proper effects

Use `/model`, mode and `/compact` through their existing native operations. Read back effective settings and observe Claude's real compaction marker and native terminal lifecycle. Cursor's unsupported compact must refuse without a prompt. **Unverified until run:** effects, busy-state clearing and native handling of errors.

### U4 — Existing conversations still work

Run live/reopened chat, interrupt and approval controls for each provider; keep native Codex and reverse ACP controls. Refresh skills during an active turn and retain output, identity and terminal result. **Unverified until run:** real interleaving, shutdown cleanup and cross-route preservation.

### U5 — Provider rules stay replaceable

Inspect dependencies and run the same actor contracts with standard, Claude and Cursor behavior. The two providers use one sender and shared native face. **Unverified until implementation:** manifest graph, generated asset integrity, no private patches or prohibited store/framework.

## Target and shared journeys

Declared candidate: a clean committed build from this dedicated branch containing current main, macOS, unchanged installed Codex0.165.0, Claude ACP0.81.0 / SDK0.3.280 / native Claude qualified at delivery, Cursor2026.10.22-27dcf5f. Use an isolated signed normal-dependency debug Host, private unused root/socket/port and normal trusted Codex home. Preserve production and configuration. Record actual build, executable versions/hashes and endpoint/session identities before model input. No global provider install or production replacement.

- **J2 commands, needed lasting:** same real client/Host/provider route; drive native compact/model/mode operations, read provider marker/settings and native terminal state, capture unsupported Cursor compact with zero send.
- **J3 preservation, existing lasting plus additions:** the ACP metadata live/history recipe and public observer are existing; add active-turn skill refresh, no-list/source-failure chat and wrapper lifecycle controls. Retain separate native Codex/reverse ACP controls. Existing projection tests alone do not establish these new outcomes.

## Coverage rows

| Need / obligations | Reference and real seam | Journey / test and passing condition | Wrong build rejected | What remains unverified until run |
| --- | --- | --- | --- | --- |
| U1 / R1,C1,V1 | Native0.165 SkillsListResponse and C1; actual provider classification → actor/hub → chooser | J1 lasting. Correct skills for Claude and Cursor before skill input, exact advertised ID and scope, valid rows preserved alongside native errors for ambiguous/unknown rows, correct cwd/binding, immediate startup and late invalidation. Source failure differs from empty. | Empty face; every command labelled skill; command title suffix spoof; native ID recomputed; wrong connection; stalled list | Installed provider/classifier agreement and first session |
| U2 / R2,C2,V2 | C2 exact content rules and native structured input; actual queue/actor/provider | J1 lasting. Fixture skill effect once; multiple distinct Cursor selections all recognized; exact prefix/suffix/UTF-8/image preservation; no file access for server references; invalid/stale/ambiguous/unsupported forms cause defined error and zero sends. | Model explanation, echo, lost image, first-only drop, wrong duplicate target, selected-steer converted to new turn | Native expansion and all real refusal paths |
| U3 / R3,C3,V3 | C3; native command request and provider effect/readback | J2 lasting. Claude manual compaction marker+successful terminal; native turn returns idle; model/mode readback matches. Cursor unsupported compact emits no prompt. Missing/failed/automatic marker cannot confirm manual compact. | Queue ack or tool title counted as compact; default model fallback; normal reply to `/compress` counted as effect | Native errors and effect confirmation |
| U4 / R4,C4,V4 | Existing adopted lifecycle; real metadata/control route | J3 lasting. Both providers preserve chat/history/interrupt/approvals; metadata is outside prose. Refresh cannot gate/restart/cancel a running turn; uncertainty never duplicates. Wrapper EOF/signals leave no child; protected native/reverse routes keep results. | Second sender, lost history, synthetic discovery prompt, reload disrupts turn, leaked child, false cross-provider inference | Active-turn timing and cleanup on exact candidate |
| U5 / R5,C5,V5 | Consumer-owned trait/dependency direction; real producer/consumer metadata shapes | Focused real TS→Rust contract and actor tests; structural manifest/cycle/asset checks, no new journey. Two implementations share actor/face and closed errors. Embedded Claude asset matches source. | Duplicate SDK/config loader, private patch, stale asset, reverse dependency, catalogue/store/framework | Final source/build graph and artifact |

## Smallest honest proof

Pure reference/token/classification rules use focused table/property tests with independent literal contracts. Cursor classifier conformance covers all four native skill indicators, all command indicators, a mimicking title, exact nested IDs, duplicates, unknown suffixes, invalid mention grammar and valid rows alongside errors. No filesystem loader is under test because none is introduced. Controlled Query promises cover Claude echo/coalescing/stale publication and real producer serialization feeds the actual Rust decoder. Actor tests force removal during a held queue wait and verify refusal before sending; sleeps cannot establish that interleaving.

The existing `.agents/skills/verify-codex-router/references/acp-metadata.md`, its `verification/observations/acp-metadata-observation.ts` and live/replay projection tests were read. Reuse their exact-target checks, public views, original PTY captures and guarded owned cleanup. Build the needed J1/J2 extensions before relying on them; no general verifier framework. A mocked provider can test malformed/unknown outcomes but cannot earn a real-provider row.

If Claude reload disrupts a turn, or actual Cursor output contradicts its qualified indicator/ID contract, return the source/observed failure to Program Design. Do not lower the passing condition, drop Cursor, silently accept unavailable skills as U1 completion, or add a new client path. Other versions/platforms and release/install proof are outside the declared target until separately qualified.
- **J1 skills, needed lasting:** extend `verify-codex-router`'s real installed-client ACP journey to drive the `$` chooser and selection for **both** providers; read native list/input plus actual provider effect and original PTY. Include native-output fixtures for exact suffix indicators, nested IDs, duplicate/unknown/uninvocable rows and a command whose title mimics a skill. Expected classification comes from the native producer contract, not the bridge classifier.
