# Historical HTML proposal verification — 2026-10-17

This file preserves dated proposal checks and earlier implementation checkpoints. Statements below such as “no Rust code changed” or “no compiled screenshots” describe their recorded time, not current status. The current draft is [PR #155](https://github.com/agent-collaboration/agent-collaboration/pull/155); see [current proof and limits](proof.html) and [feedback checklist](owner-feedback.md).

Scope: a proposal explaining one standardized TUI dialog component system, with interactive illustrative terminal views. This is not an implemented Rust/iocraft change and does not prove any Router session launch.

## Binding and source

- Branch codex/sessions-ux-redesign, clean origin/main base ffced1c1c3c84e0ca41701fb9eb4326acea2451e.
- Demistify public /v1/ binding for both theme/runtime; release1.2.2, sourceCommit69d403c91e39a4cd768012d748d985586d4b43c9.
- Matching authoring guide, tree and map references read. Inline document prose/HTML, ex-tree outline, ex-flow arrows, ex-card ownership explanations and ex-tabs storyboard. The interactive terminal is one isolated HTML custom element with synthetic data, not a reconstructed app screenshot.
- Component tree, shared primitive roles, responsibilities, sizing/margins/border/action/keyboard rules, source findings and preserved scope are explained. Quota is included by the later owner correction. No product implementation is included; Activity/board/Host work remains outside scope.

## Checks observed

- node --check tmp/sessions-ux-proposal-library/prototype-script.js: exit0 on final inline script.
- HTMLParser check:16 unique stable IDs, four local source links resolve, no duplicate IDs.
- git diff --check: exit0. Only the new docs/wip proposal folder is untracked; no product edits.
- Actual direct-file opening observed in Safari and Chrome. CDN document styles and component runtime loaded successfully.
- In Chrome, opened the inline machine list, selected Sunbook, observed retained New session form and updated machine/directory explanation. No extra picker page.
- Switched terminal preview to48×24; visually observed content-sized frame, one title, readable fields and Cancel/Start together in a horizontal row. Also observed76×30 compact preview and120×36 storyboard.
- Enabled component-region overlay; AX output identified DialogHeader, DialogBody and DialogActions. Observed the component tree with all header/body/action children visible in actual rendered page.
- Browser screenshot of the actual HTML page saved as proposal-browser-wide.png. It is a browser capture of this proposal, NOT a running iocraft screenshot. Initial unreadable map-layout capture retained only under ignored tmp; final page uses the rendered outline.
- Full-page capture shows normal-path arrows connecting Sessions, New session and Start. Source links resolve to the owning Rust files.

## Limits and corrections

- Matching source Demistify checker was not found in inspected local checkouts; its check command was not run. Parser/syntax checks do not replace it.
- Safari native AX interaction returned elementHasNoFrame; successful interactive checks used Chrome. No permission or browser security setting was bypassed.
- The first library map layout was visually blank; changed to the documented outline and observed actual hierarchy rendering. No map-render success claimed.
- Modified terminal shortcuts, all form/focus permutations, source-error simulation and every storyboard interaction were not exhaustively exercised. This proposal is not component runtime acceptance.
- Real product verification still requires the actual iocraft component and every caller, keyboard/focus/overflow cases and compiled wide/compact/narrow screenshot journeys.
- HTML includes no real session data or provider calls. Start/Resume/Fork simulate text outcomes; Copy ID leaves the system clipboard unchanged. Library assets require internet; no page was publicly published.

## Current revision — Quota, all menus and structural design

The owner explicitly chose one New form then Start and later required all menus/filters, reusable searchable machine/provider choices, preserved folder scopes/features and Quota layout cleanup. The earlier provisional/excluded scope is superseded.

- Current index.html has24unique explicit IDs and five checked local links; final inline JavaScript passes Node syntax check, exit0. The new design.html has16unique explicit IDs; its three embedded Markdown islands compare exactly to current Requirements, Specification and Program Design source files.
- Actual Chrome observation:48×24 New form → open machine choice → type sunbook → Enter retains the same form with Sunbook. A no-such-machine query visibly shows no matching choices; Escape closes the choice and retains Sunbook/form/Cancel/Start.
- Actual Chrome observation: narrow Account options → Inspect resets → Continue replaces the same frame with Review/Confirm states, with header Help and Back/primary actions. These buttons have no real account side effect. Current examples also expose credit policy and weekly floor; their full interaction permutations have not been exercised.
- Actual rendered page inspected for full invented account names, provider beneath, compact62.5k amounts, one name/provider/metadata content column and corrected singular reset label. This establishes browser proposal appearance only.
- Current proposal-browser-wide.png is an actual Chrome full-page capture of the current HTML,5,030,499bytes. The old pre-Quota/menu capture is held under ignored tmp and is not current proof. First new capture produced no file after DevTools was closed; the corrected capture kept DevTools open until the file arrived.
- Actual structural-page component tree and New sequence render as diagrams; entity-binding and R coverage tables render as tables. Unescaped enum pipes initially broke two binding rows; escaped them and inspected the corrected rendering. Native arrow labels shortened after they overlapped card text. Embedded source equality alone was not treated as diagram proof.
- Matching Demistify source embed/check CLI remains unavailable. This page uses documented inline Markdown islands generated from exact source and directly compared; it does not claim a matching CLI check or source-embed command execution.

## Bounded iocraft feasibility experiment

Ignored design spike: tmp/design-workflows/2026-10-17-tui-sizing-probe, exact iocraft0.9.2, Rust1.98.1, private system-temp target. It does not alter product code or claim permanent regression coverage.

- Fixed viewport baseline: short3-row body yielded18frame rows, action at zero-based row15; exit0.
- One real ScrollView with parent-owned measured content height: short shrank to9rows/action6;40rows capped at18/action15; End displayedrow40; resize14kept actions/scroll offset; shrinking returned to9. Exit0. Initial probe expected the wrong row index and timed out; corrected against actual rendered geometry, without a product/proof change.
- Auto-width frame around existing ScrollView compressed the wider body to19columns and wrapped it; it did not establish content width.
- Native in-flow clipped viewport with a non-shrinking content wrapper: short10rows/action6;40rows capped18/action15; actual content measurement40, End displayedrow40; resize14/action11 retained offset; shrink returned10rows/action6. Exit0. Without the wrapper, the measured content was constrained to12 and End could not revealrow40; that failed variant was not adopted.
- Native intrinsic-width case:59columns, complete wider body line visible, exit0. It used the same in-flow wrapper and ordinary Text measurement. No duplicate tree, provider I/O or Router process was involved.
- This settles a bounded layout feasibility question only. Product focus reveal, all keyboard/mouse inputs, nested field content, Unicode positions, clipping under every sizing policy, current tests/quality, real-TUI screenshots and independent review remain unverified.

Structural-realization confirmation is pending on the current component view, entry-to-effect path, binding table and trace table. Verification Design and the implementation plan follow that confirmation; no persistent Implementer/Verifier or completed product gate is claimed yet.

## Latest owner correction — glyph counters

The annotated Quota header asks for glyphs/icons in place of some repeated labels. Requirements U11, Specification R10, Program Design's Quota counter mapping and the canonical feedback list now carry that correction. The structure confirmation remains pending; this change is authorized proposal refinement, not permission to start implementation.

- Current source inspected: quota_status_projection.rs counts typed Usable/Reserve/Blocked/Unknown/Excluded availability separately from freshness and sums active clients. quota_title_status formats serving/freshness in the title row. Glyphs preserve those distinctions; generic UI code must not derive meaning by parsing those formatted strings.
- HTML header now shows Σ total, ✓ usable, ◇ reserve, ⊘ blocked and × excluded. Operational group shows ⇄ active clients, ● fresh, ◷ stale, ? unknown freshness. Rows reuse ⇄ clients and ↻ reset credits. Route/provider/window/unit labels stay text. The Help legend defines each group and distinguishes unknown availability from unknown freshness.
- Invented fixture counters derive from the example records rather than a hardcoded sentence:7total,4usable,1reserve,1blocked,1excluded;3clients,5fresh,1stale,1unknown freshness. These are not the screenshot's live account observations.
- Actual Chrome wide and48×24 inspection observed all pool counts, header Details/Help, names/providers, readable62.5k and spaced row counters. Narrow account statuses initially ran into the name; moved the visible status beside the provider at narrow widths and rechecked actual accessible row text. Glyph counters use normal/secondary colors; selection remains yellow.
- Actual Quota Help click opened the same shared prototype frame with the full glyph legend, a scrollable body and visible Back action. Back returned to the retained narrow Quota view; switching back to wide kept the glyph summary.
- Current proposal-browser-wide.png is the actual final Chrome full-page capture,5,068,385bytes. The pre-glyph capture is retained under ignored tmp. It is HTML proposal evidence, not a compiled terminal screenshot.
- Final Node syntax check exit0;25unique explicit proposal IDs; authored local links resolve; all3embedded design islands match current Markdown. Demistify release metadata rechecked: /v1/,1.2.2, source69d403c91e39a4cd768012d748d985586d4b43c9. Matching source CLI still unavailable. No provider/model call, account effect or production action.
- Real terminal font/cell width, fallback readability, actual glyph styling/alignment and product interaction remain unverified. Product items stay unchecked.

## Current revision — preserve columns and default to bounded percentages

The owner corrected both the Quota grouping and sizing default. This section supersedes earlier proposal descriptions of full-width account names, side-by-side usage windows and content-fit defaults; earlier observations remain historical evidence only.

- Current baseline inspected: remote main and local HEAD both ffced1c1; Homebrew CLI reports0.1.96. Row renderer146–210 has Account / Status-and-counts / Quota grouping. Actual screenshot supplied by the owner matches it. Fresh direct Ghostty observation was denied by the computer-use tool; no alternate capture/app route was used and no new live-terminal inspection is claimed.
- Revised HTML keeps name/provider in Account; state, client count, reset credits and balance/freshness in Status; reason, Weekly,5h and pace in Quota. Weekly and5h remain vertically stacked. All row/header grids use the same column allocation and reserve scrollbar space. Long names wrap rather than being capped at13/17cells. Narrow status words use defined glyphs (★ preferred, ✓ available) with accessible labels and the contextual Help legend.
- Both dialog axes now DEFAULT to clamped percentages. Editable example:80%width with42–88column bounds;60%height with8–24row bounds; available space after margins is the final cap. Content and full-screen remain explicit supported modes. Reset restores both percentage values/modes and the example's own initial screen/preset.
- Actual Chrome inspected the revised wide Quota grouping, full wrapped names, provider labels and compact balances. At48columns the same groups remained; text wrapped inside them rather than being redistributed across full-width lines. Opened narrow Account Details: one bounded frame, visible header/Help and Options/Close actions, with overflowing body scrolling. These are browser proposal observations, not terminal geometry assertions.
- Final inline JavaScript syntax exit0.26unique explicit proposal IDs,16design-page IDs; authored Markdown links/whitespace checked; all3embedded governing sources exactly match current Markdown. Removed contradictory old default/grouping prose from current governing documents and kept the supersession in the feedback list.
- Current actual full-page Chrome capture: proposal-browser-wide.png,4,833,548bytes. Previous glyph-only arrangement retained under ignored tmp. Matching Demistify checker remains unavailable. No product files, runtime accounts or provider/session actions changed.

Remaining: real compiled implementation, permanent behavior tests/quality, paired Verifier observations, terminal font/cell alignment, actual app screenshots, independent review and current-head CI. The proposal revision does not close any product gate.


## 2026-10-18 — accepted glyph-label refinement and fourth design source

Owner confirmed the current structure and authorized development, adding glyph+label+count on wide headers, spaced middle-dot separators, and reset symbols. Updated Requirements/Specification/Program Design/checklist/HTML, then authored the separate Verification Design. Node syntax check exited0; four embedded Markdown sources compare exactly. Actual Chrome refresh displayed wide labeled counters, spaced dots, original three-column grouping and ↻ row reset counts. The prior proposal-browser-wide.png predates this refinement and is historical. Browser prototype observations are not product proof. Independent four-document review is running in a read-only reviewer session whose identity is retained privately. No Rust code changed yet.


### Current browser delta and proof presentation binding

Chrome directly observed narrow48×24 Quota after the accepted status-vocabulary correction: header glyph/count counters keep spaced middle dots, labels collapse, three account groups retain their starts, full name/provider wrap, preferred★ remains distinct from available✓, resets show↻. Wide counterpart previously observed labels. These remain invented-data proposal observations. Source-built VHS/ttyd executables started for preflight only; their path names alone do not establish versions. Demistify /v1/ release metadata now reports1.3.0/source5255d4199a165d95b9bcb5a4c573ebb649e085e1. New proof gallery will use the matching pictures reference, original PNG figures and ex-gallery facets; browser interaction/decoding still required once actual screenshots exist. No current compiled screenshot evidence yet.
