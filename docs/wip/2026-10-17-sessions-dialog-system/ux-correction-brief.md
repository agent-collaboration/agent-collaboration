# UX correction brief — readable shared dialogs

This corrects implementation of the existing Sessions/Quota component design after the owner's annotated screenshots. It does not add a workflow or change account/session/action identity. Current screenshots fail overall UX acceptance. The separate UX Verifier opened all96 originals and both annotations and returned seven anchored findings; runtime marker passes remain narrower evidence.

## Shared visual contract

1. One cyan task title with subdued Help. Put attributable account/session identity in a shared context area outside the scrolling task body; preserve the captured target and make its full name accessible. A consequential action must never rely on the obscured background row for identity. Caller owns the identity; the shared dialog owns its placement. Extend the existing DialogFrame with a context slot when needed; do not create a second frame or duplicate domain state.
2. Compose context, task fields, necessary notice and actions as distinct groups. Ordinary labels are white/normal, important values readable, section headings white/bold only for a distinct section, supporting text grey. Yellow indicates current focus/selection; use existing semantic status/error roles with explicit words and restrained emphasis. A success message must not look like a blocking warning. Keep stable field columns and reserved selector gutters; use stacked fields when the narrow width requires them.
3. Preserve independently clamped percentage width/height. A short body's natural height must not push actions away: actions follow content with one deliberate gap; remaining space can sit below the group. Long bodies cap and scroll while context/header/actions remain usable. Adapt default padding/gaps to constrained height. Correct the test that currently requires fixed-bottom actions against this existing R4 contract; replace it with meaningful short/long/resize assertions, not a deleted proof gate.
4. Keep all existing capabilities, guards, queries, folder scopes, provider support and keyboard/pointer access. No data, protocol, storage or credential change follows from this correction.

## Quota content and controls

- Status summaries use clear text labels and dot separators, never an unexplained glyph-only fallback. Keep pool classification distinct from data freshness; qualify their different unknown states. Wide/medium show readable named groups. On a constrained screen show total/usable and nonzero problems prominently, with the complete current counts and definitions reachable in existing contextual Help; preserve all values. This trades one Help action for detailed counters on small screens, keeping accounts and actions readable. Use glyphs only where they reinforce a familiar meaning, such as providers or a labeled refresh/reset action.
- Reset landing: retain account/tab context and the actual Inspect action; remove the repeated task heading and routine 'Inspect reset eligibility' / 'No reset is consumed...' blob. Keep unavailable/pending reasons when relevant. Put routine instructions in contextual Help.
- Credit policy edit: one 'Credit usage' title; account context, a concise balance where relevant and a 'Use credits' field with the existing Allow/Disallow meaning. Remove the duplicated heading and the two routine eligibility paragraphs from the form; preserve that guidance in Help. Separate an applicable weekly-floor/provider warning from the field and from Cancel/Save. Keep unknown/failure/pending information truthful.
- Confirmation: show the No/Yes choice once as one labeled choice group. Reuse existing controls and intent handlers so both pointer selection and Left/Right work; Cancel and the final confirmation remain a separate action group. Preserve default No, No+Enter cancellation, disabled Yes, revalidation and uncertain-outcome guards. Do not merely delete the pointer-capable No/Yes controls while leaving static text. Keep a short explicit irreversible-consumption warning with the decision, plus the chosen credit/expiry/eligibility information reachable in the body.
- No-match: keep the active field, query and result attributable together; remove redundant draft/current-summary competition. Select must be visibly unavailable when no available choice is focused, and cannot commit a different value. Apply this shared rule to Sessions and Quota choice fields.
- Reset inventory items must separate the marker, voucher ID and description with real layout gaps; no 'next[voucher-1]Synthetic...' run. Distinguish fixture names from production copy.

## Ownership and proof

Main owns the presentation corrections and permanent product tests in codex-router-tui, Quota presentation and Sessions picker; declare exact files before editing and split by responsibility if needed. Runtime V owns the existing verification skill/drivers and original capture runs. The fresh UX Verifier owns read-only visual judgment and test-gap findings. Root owns this brief, governing docs, HTML, integration and acceptance.

First finish the already authorized controlled48x24 phase/focus comparison. Then implement this consistent correction within the existing component owners. No generic approval loop for routine file choices, causal diagnostics or bounded corrections preserving this contract. A public/persisted/security/ownership change, changed authorized behavior or weakened proof still returns to Root.

Required proof: focused causal tests that exercise the actual composed dialog, not only a detached body; proper disabled-action and pointer/keyboard outcomes; actual compiled originals for Sessions and Quota at120×36,76×30,48×24, constrained48×12 and phone-width40/32 cases (use actual declared row dimensions). Include short/long bodies, search/no-match/recovery, identity during editing/confirmation, full lower content, pending/error/disabled states and actual resize. Strengthen semantic oracles when wording changes; never remove an obligation to get green. Every fresh image gets both runtime observation and cold-reader UX assessment. Keep annotated before/after originals in the HTML gallery with exact source/build/grid/result/limits.

Actual iPhone terminal operation remains a separate acceptance case: desktop grids/resize and phone-viewer JPEGs are not device proof. Establish the available device/terminal route through Sun when the corrected candidate is concrete; do not claim the case passed without observation. Existing parked debug Router, provider, production and credential boundaries remain unchanged.

Source basis: actual wide/medium originals in normal30-color-run-image-manifest.json and normal30-wrap3-run-image-manifest.json; compact-pixel36-run-image-manifest.json; dialog.rs117, dialog_body_viewport.rs159, dialog_tests.rs363, quota_account_options_rendering.rs60/241/350, quota_dialog_actions.rs112/133, quota_status_symbols.rs147 and the latest owner-feedback.md section. This applies the reader principle's 'adapter seam' move: use names the reader understands, and say each thing once.



## Shared pointer activation

The mounted narrow filter and shared SearchSelect observations establish that an invisible field/option could receive a click intended for a fixed action below the body. Preserve the existing single input owner: the viewport supplies its visible band, the existing local-event helper rejects clicks outside it, and one DialogButton primitive applies that policy to button activation. DialogAction, SearchSelect and the scoped New/Fork field controls use it. Preserve the pinned native child layout and activation semantics, caller-owned keyboard focus, captured identities and all eligibility guards. No event-order assumption, library fork, dispatcher or alternative renderer.

Closure needs an immediate no-effect assertion after the hidden click, separate visible-control positives, scroll/resize, blocked scope and keyboard parity. Existing no-match coverage is necessary but insufficient because it renders no option buttons. Fresh screenshots still establish appearance separately from these interaction outcomes.

### Concise active-page titles

Quota account options use “Options · Resets” and “Options · Credits” uniformly so the purpose and active tab remain visible beside Help at32 columns. Captured account identity supplies the account context. Keep the shared header behavior and existing tabs/actions; do not add a width-specific renderer or accept an inactive body-tab label as proof of the active page. Fresh originals and the full journeys remain required.


## Current32 cold-review corrections

The actual31 original234545 corpus and independent source/UX reviews require fitting row-status words to survive without a glyph when necessary, full natural inventory-range captions to wrap, and an ellipsis for shortened captured names while the full tag stays visible. Existing contextual Help already owns the full captured label; prove opening, scrolling and returning with the same draft/selection/account rather than adding a dialog or identity state. Keep static output intact.

Replace vague Help exit prose with “Exit Quota unless a reset is being applied,” matching the existing commit guard without changing behavior. Add original observations for post-refresh balance and the Credits fact/policy boundary, preserving all previous scenes/negative checks. Credits action wrapping remains allowed byR4; Edit on its own line is an optional preference, not a new redesign requirement.
