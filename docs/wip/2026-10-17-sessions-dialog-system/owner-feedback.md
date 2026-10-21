# Owner feedback — Sessions, Quota and shared TUI components

Canonical checklist for this redesign. Origin: this owner conversation, 2026-10-17. A checked item needs linked evidence. The HTML is a proposal, not product completion. Latest owner corrections override earlier proposed exclusions.

## Current delivery status — 2026-10-21

**Not PR-ready yet.** Latest C8 product changes are built and scoped tests pass. Current compiled screenshots, successful provider composition on both debug machines, final review and current-head CI are still required. Published C7 pictures remain historical.

- [x] Implemented inline choices in the shared value column, retaining labels, searchable selection and fixed selection gutter. All existing folder/provider filters remain.
- [x] Added **Copy path** beside **Copy session ID**, using the complete captured path through the existing clipboard owner. Empty paths have no action. Actual OS clipboard delivery is not yet proven.
- [x] Implemented sentence-case action labels, aligned helper text, and full captured title/path/ID in Details. Mounted component measurements cover all six grids, including ordered scrolling at short height.
- [x] Shared semantic colors now reserve yellow for selection/focus; warnings use orange, errors red, success green and secondary text grey. The older yellow-warning instruction is superseded; warning meaning and blank-row spacing remain.
- [x] Scoped source gates: shared TUI 27 passed; Agent default library 454 passed / 8 ignored; Quota CLI 573 passed / 7 ignored; helper 38 passed; copied normal component 14 passed. Applicable strict lint/format/size checks pass. These are source/component results, not visual or vendor acceptance.
- [ ] Refresh original compiled screenshots for both screens and dialogs at the required grids, inspect them independently, and publish them in the component gallery.
- [ ] Complete current debug-Host New/Resume/Fork/shared catalogue and Cursor actions on both computers, with exact identity and owned cleanup. First local Host attempt ended before NativeNew; cleanup passed. The corrected real provider startup is admitted for the next bounded observation.
- [ ] Complete current-source independent review, publish scoped changes, update the PR description and pass current-head CI. PR #155 remains draft at the earlier published commit.

Evidence: `tmp/tui-implementation/c8-runtime-boundary-target-handoff.json`, `tmp/tui-implementation/c8-runtime-boundary-final-quality.json`, `tmp/tui-pr-draft/c8-native-endpoint-lead-disposition.json` and `tmp/tui-pr-draft/current-debug-proof-matrix.md`. These local evidence paths are task-scoped and may expire.

## Earlier evidence — 2026-10-20

- [x] Actual presentation-html `ex-gallery` now contains 20 current Sessions originals and nine retained Quota examples, with Screen/View/Build filters and separate shared journeys. Original bytes and build identities are preserved. [Open Sessions first](quota-gallery.html#sessions).
- [x] Current Sessions at 120×36: 20 image checks and three wrong-image controls passed; independent review personally opened and hash-checked all 20 originals, with no confirmed visible P0–P3 finding. Child 0, provider requests 0, state/secrets unchanged and owned cleanup complete. Synthetic inventory through the real UI, not public/provider action proof. [Evidence](proof-assets/2026-10-20-sessions-wide/evidence.json), [review](proof-assets/2026-10-20-sessions-wide/independent-ux.md).
- [x] Requested blank row immediately above the yellow saved/live reset-count warning is implemented at the existing renderer. Causal render proof fails old code and passes the repair; warning text/color/actions remain. Latest full CLI: 573 passed / 0 failed / 7 ignored; 13 copied-default checks and strict Clippy/format/size pass.
- [x] Reset-confirmation No now returns to the concise factual body “Eligibility / Not checked”. A permanent actual component transition covers six grids, and the bounded independent source review is clear.
- [ ] Replacement original Quota screenshots must still show both the repaired No return and the warning spacer. Source/test results do not close this visual gate.
- [ ] Combined current capture failed in Quota 120×36 at native Disabled accounts / Shown field association after Sessions passed. Ten later tours remain unrun. The failed Quota tour has no successful effect receipt; its external effects remain unknown. Owned cleanup is complete; the failed archive is preserved.
- [ ] Meaningful glyphs may accompany readable text, backed by typed semantic enums and one standard visual language. The current icon sketch is a proposal only; no scattered literal or decorative glyph mappings are authorized by it.
- [x] Retained Quota 593 build at 32×24: 42 positive image checks / 12 wrong-image controls pass, 69.24s, wrapper/driver/child 0, zero fixture effects and cleanup clear. Independent review opened/hash-checked all 42 originals and closed P3-32-1.
- [x] Six earlier Quota sizes independently inspected: 32×24, 40×24, 48×12, 48×24, 76×30 and 120×36. Wider e873 originals retain their build identity and failed-reader history. This is explicitly reused earlier evidence, not all-grid current capture completion.
- [x] Owner scope: narrow WIDTH and short HEIGHT desktop viewports are the accepted iPhone proxy; actual iPhone hardware testing is not required.
- [ ] Narrow/short current Sessions, current Quota Filters and pending/error/disabled states, live same-child resize, broader native/provider/two-machine/public paths, fresh-checkout reproduction, final whole-PR review and current-head CI remain open. No merge-readiness claim.
- [ ] Browser rendering of the component gallery remains unverified after the retained local-file URL policy refusal. Static references, component structure and original asset hashes are checked.

## Earlier verification receipts — historical and superseded

- [x] Latest 593 build: fresh 32×24 Normal journey passed 42 positive / 12 wrong-image checks in 69.24s, child 0, zero fixture effects and cleanup. Independent reviewer opened/hash-checked all 42 new originals and CLOSED P3-32-1. [Focused gallery](quota-gallery.html), [complete new storyboard](proof.html#c7-phone-pace-current).
- [x] Latest source gates: one full CLI command 571/0/7 ignored; 39 affected tests, 11 copied-default checks and strict quality pass. Independent five-file source review is clear.
- [x] Owner scope update: actual iPhone hardware is not required. Narrow-width and short-height desktop viewports are the accepted proxy; older hardware-open notes below are historical.


Last observed product component `27cbcfac…`, source snapshot `b2c04ebf…`. [Original screenshots, exact reports, hashes and limits](proof.html#c7-current-normal). Older dated run failures below remain historical; they are not relabelled as passes.

- [x] Current 48×12 and 32×24 Normal journeys: 42 scene checks and 11 wrong-image controls per grid; wrapper/driver/child exit 0, zero fixture effects and all 58 recorded processes/profile absent per run.
- [x] Separate cold UX observations: all 42 originals in each grid personally opened and hashes verified; no confirmed P0–P3 visible defect in either bounded corpus. Root read both full reports and opened decisive current images.
- [x] Separate 40×24 still review: all 42 retained originals personally opened and hash-verified; no confirmed P0–P3 visible defect. [Originals and full assessment](proof.html#c7-current-visual40). This does not promote the failed automatic run or its unrun checks.
- [x] Independent 48×24 cold UX review: all 42 originals opened at native size and hash-checked. It found two P3 readability issues; Root independently verified them. This brings the current reviewed corpus to 168 originals, without promoting the failed 48×24 automatic run. [Report, annotated original and exact limits](proof.html#c7-current-visual48).
- [x] P3-1 and P3-2 visually verified closed at new build e873, 48×24: the separate UX Verifier opened and hash-checked all 42 new originals; no new confirmed P0–P3 visible issue in this corpus. Root read the full report and opened decisive before/after originals. [New originals, independent assessment, code evidence and exact failed-runtime limits](proof.html#c7-density-p3-current). This is not all-grid or whole-PR closure.
- [x] New e873 48×12 Normal journey: 42 positives/12 wrong images pass, wrapper/driver/child 0, zero fixture effects and independent cleanup; separate cold UX review personally opened/hash-checked all 42 originals with no confirmed P0–P3 visible defect. [Originals and full limits](proof.html#c7-density-short).
- [x] New e873 32×24 Normal journey passes 42 positive/12 wrong-image checks with wrapper/driver/child 0 and retained cleanup. Independent review opened/hash-checked all 42 new originals; one narrow readability issue remains below. This does not substitute desktop cells for physical-device proof.
- [ ] Accepted P3-32-1: eliminate the broken `unavaila / ble` word and orphan fragment in 32-column browse using the existing responsive presentation, preserving the three groups, full names/provider, all facts and meanings. Count/label stacking where the physical width cannot fit both is an explicit tradeoff, not a one-line mandate. Root personally verified the exact current image; Main owns bounded repair after current archive collection.
- [ ] Complete the new density correction’s remaining sizes and behavioral proof. Old 27cb screenshots remain their earlier build; the new 48×24 runtime remains failed at a strict native Floor identity check.
- [x] DENSITY-P3-F1/P2 independently VERIFIED CLOSED: modal top geometry is bound before its matching bottom; malformed-bottom negatives fail closed while proper/CJK-underlay positives pass. Final 37 affected tests, strict Clippy/format/size pass. This is a separately bound later test-helper repair; e873/full565/default9 are not relabelled. [Causal proof and independent recheck](proof.html#c7-density-p3-current).
- [ ] Accepted P3-1: keep related reset counts/units and credit/quota values easier to scan at narrow widths, preserving Account / Status / Quota, full identities and every fact. The product correction is implemented and independently visually closed at48×24; other-grid proof remains open.
- [ ] Accepted P3-2: remove the wrapping empty pace graphic when burn is unavailable; retain explicit unavailable meaning and real measured meters. The product correction is implemented and independently visually closed at48×24; other-grid proof remains open.
- [x] Prior initial Options P2 visually closed at 48×12: Resets immediately shows inventory facts; Credits immediately shows usage policy. Navigation, title, captured identity, facts and actions remain. Before/after originals and independent closure are linked above.
- [x] Keyboard lower-browse paging: causal mounted regression and current compiled lower-credit/pace/return sequence retain the selected account and restore the initial view.
- [x] Current CLI quality: 560 passed, 0 failed, 7 ignored across 11 targets; strict Clippy, format and size exit 0. Nine exact checks against the default compiled artifact pass. [Commands, immutable raw logs and source-review scope](proof.html#c7-paging-options-source).
- [x] Latest bounded code/DX review P3 and supported-key proof gap independently VERIFIED CLOSED: complete named eligibility or voucher content accepted before scrolling; actual chrome-only entry rejected; voucher reachability separate. Default focused tests 1/0 each and fitting lint/format/size pass, separately bound from original full560/default9. [Independent recheck and exact later evidence](proof.html#c7-current-normal). No runtime code defect or whole-PR acceptance was inferred.
- [ ] Complete the remaining size/interaction proof after the two accepted readability corrections. Earlier 40×24 and 48×24 runs retained all 42 originals but failed image recognition. The 76×30 attempt failed a tape synchronization wait before checkpoint export; child/effects are unknown because receipts are absent. 120×36 remains unstarted. Bounded apparatus repairs and qualification are underway; no failed run is promoted.
- [ ] Observe pending/error/disabled states and same-child resize at wide/narrow sizes, with original screenshots and unchanged identity/draft/decision semantics.
- [ ] Provider/two-machine/public Sessions actions, fresh-checkout developer reproduction, final whole review and current-head CI. None is established by the two synthetic Normal grids.
- [ ] Proof HTML browser rendering remains unverified after the retained file-URL policy refusal. Static image/hash/reference checks pass; no bypass or rendering claim.

- [x] All six e873 desktop grids collected: 252 originals independently opened/hash-checked. New 76×30 and 120×36 reviews found no confirmed visible P0–P3. Their automatic selected-name checks remain failed, with later checks unrun and no Normal receipt. [Wide pictures and complete limits](proof.html#quota-current-examples). The 32-column correction still requires fresh proof.

## Latest owner corrections — intentional and understandable UI

The owner rejected the current pictures for visual clarity even where the automated screenshot checks passed. The two top strips of glyphs are not understandable, and the Resets and Credit usage dialogs put repeated headings and routine prose beside fields, warnings and actions without sufficient distinction. Earlier layout-only verdicts do not close this feedback.

- [ ] Use one consistent design language across Sessions, Quota and every dialog: intentional color accents, text weight, spacing, grouping and indentation. Distinguish displayed facts from editable fields and actions; reserve the selector gutter consistently.
- [ ] Make status summaries understandable without memorizing unexplained glyph sequences. Preserve the information and use clear names, restrained icons and consistent separators; do not present two opaque telemetry strips as the primary header.
- [ ] Remove repeated headings and routine explanatory blobs from the decision path. Use concise labels/values; put supporting explanations in contextual Help. Preserve necessary warnings beside the action they concern, especially reset confirmation.
- [ ] Visually separate identity, task fields, warning/status and actions using the shared components. Labels, values, supporting text and actionable controls must not run together.
- [ ] Inspect actual screenshots at wide, medium and phone widths, including widths below 48columns when the terminal uses them. Test wrapping, horizontal overflow, readable names, visible actions and navigation. A static48-column run or desktop resize does not establish actual iPhone-terminal behavior; the physical-device observation remains explicitly open.
- [ ] A separately commissioned UX Verifier must personally inspect the original pictures and owner annotations, challenge the offered tests, and return concrete per-screen clarity findings. Existing runtime Verifier continues input/scroll/resize proof. Passing marker checks alone do not mean usable or visually accepted.

## Historical verification checkpoint — 2026-10-19

- Latest corrected-wide capture exported 31 original PNGs at 120 × 36; the Lead and runtime Verifier personally opened all 31. Fixed captured identity and result hierarchy are visibly improved; full outer bottom edges are present. The run remains FAILED after 27 positive checkpoints at the confirmation spatial gate, with eight negatives unexecuted. The independent UX Verifier also opened all 31 and confirmed a P2 Credits fact-group spacing defect; hierarchy stays partial. [Corrected originals, annotation and limits](proof.html#c7-corrected-wide). Narrow/phone/Sessions/device and final provider gates remain open.
- The frozen Quota/shared package gates passed566/0/8ignored across13targets, exit0; strict Clippy and formatting passed. Root read the raw totals and retained linker warning. Later Sessions/shared changes need their own current quality results. All 8 focused Sessions choice/Cancel regressions are reported green; fixed-context tests exposed two further defects being corrected. These scoped tests are not whole-UI acceptance.

- Historical C7 pilot produced31 new compiled originals, personally opened by the Lead and separate UX Verifier. Six original clarity findings are visually corrected at120×36; hierarchy remains partial because Floor/Details repeat account identity and the result still has a false action cue. The outer surface frame is clipped, with cause unproven. The run exited101 on a visibly present title that OCR joined as Weeklyfloor; no run pass is claimed. Pointer selection and strict product lint remain open. [New originals, annotations and exact limits](proof.html#c7-wide-preliminary). Phone, actual iPhone, Sessions and broader final gates are still required.

- The paired Verifier personally opened all 36 final Quota originals: nine scenes each at 120×36, 76×30, 48×24 and 48×12. The Lead opened six changed wide originals and verified the other 30 exactly match previously inspected originals. [Passing compact journey, source/build, actions, visual review and limits](proof.html#quota-current-checkpoint). This is a verified subset, not whole-UI acceptance.
- The fresh compact run passed (1 test, 0 failures): 36 ordered image checks, 16 wrong-image controls, four read-only effect receipts, zero provider requests, unchanged fixture state/secrets and four normal child exits. Independent cleanup found all 64 recorded PIDs and the driver absent, four private profiles and fixture roots absent, and no errors. Small-screen no-match and Shown → Hidden focus are visible. Prior checker failures remain preserved. The final oracle retains strict field associations and measures original label/glyph pixels when OCR omits a chevron.
- Product repair `d7bb5ce8` passed four affected packages: 1,264 passed, 0 failed, 51 ignored; strict Clippy and formatting passed. Commit `f0769c2f` also repairs the private SQLite test cleanup from the prior CI failure. Its focused tests passed 2/2, native integration 4/4, and strict Clippy passed. All five GitHub checks passed for `f0769c2f` in run `39275977331`. This gallery-only successor needs its own current-head checks. These results do not establish the missing UI/provider outcomes.
- The latest actual native Claude New succeeded without a prompt and was stopped. Loaded discovery failed strict timestamp binding; the actual mismatch was not retained, so a product cause is unproven. A later own stopped Stored query matched the UUID, handle, cwd and adjacent vendor timestamps; its whole-run nonpass remains retained.
- Sun parked both machines' existing debug-profile port holders until the owner decides this evening. No signaling, adoption, shared Codex config port write, alternate profile, SSH Cursor/Keychain retry or production change. The instrumented Loaded and Sunbook-local provider observations remain not started.
- The Verifier's until-done 20-minute wake is active and delivery was observed accepted. Root's reminder deliveries were rejected before submission; Root canceled that undeliverable recurrence and reported the gap to Sun. Do not infer continuation from saved reminders. Independent work continues. Complete Sessions/Quota dialogs, lower scrolling, current resize, long names/credits, both-machine provider actions, independent review and current-head CI remain required. PR #155 stays draft and unmerged.

- [ ] Close the confirmed reset-confirmation color defect: selected No/Yes yellow, the other ordinary choice white, disabled Yes grey. Both pre-repair choices are yellow. [Thirty actual wide originals and annotated finding](proof.html#quota-normal-wide-observed). The causal cell-style test and fresh corrected screenshots are required; no whole-run pass is claimed for this checkpoint.

## Scope and delivery

- [x] Start a separate branch from freshly fetched origin/main: codex/sessions-ux-redesign at ffced1c1c3c84e0ca41701fb9eb4326acea2451e; isolated worktree created and clean base verified.
- [x] Deliver an HTML proposal explaining the shared component system with diagrams and wide/compact/narrow illustrative views: index.html. Owner explicitly likes the new visual direction.
- [ ] Include Quota in the same cleanup and component system. This supersedes the earlier proposal's exclusion of quota account-layout changes.
- [ ] Preserve every existing feature unless the owner explicitly asks for its removal. An omitted mockup control is not authorization to remove it.
- [ ] Complete the invoked design and implementation-goal workflows, with Lead-authored governing inputs, independent review and a paired Verifier. Default delivery terminal is PR-ready/unmerged; no new merge or production-replacement authority inferred.
- [x] Use Workers and persistent Sidekicks for bounded assigned work; Lead owns design, plan, integration and acceptance. Evidence: native Researcher Worker source inventory; persistent product Implementer and paired independent Verifier, settings read back; exact identities retained in the private work trace. This checks assignment only, not delivered behavior.
- [ ] Finish the current Sessions/Quota UX before returning to Activity/Following, boards or Host work.

## One standard component system

- [ ] Every TUI dialog, menu and filter consumes the shared component system. Latest owner instruction: “all menkusl and filetsr shodkul kuse the dailog system”. No machine chooser, provider picker, account-options menu, editor or confirmation draws its own frame and keyboard policy.
- [ ] Only ONE active dialog level. Choice fields search/select inside the current dialog; never nest a dialog for machine/provider/other choices.
- [ ] Standard header, content, actions, focus, keyboard routing, scroll, border, padding and margins. Compose content rather than separate page implementations.
- Superseded agent-authored default: content-driven sizing was proposed as the default. The owner now requires clamped percentage sizing for both axes.
- [ ] Width and height DEFAULT to percentages with independent min/max clamps and the physical viewport as final cap. Explicit content-size/full-screen options remain supported; the HTML default and Reset must match the new default.
- [ ] Width and height independently accept content size, x%/y% of available screen, and preferred size clamped by min/max bounds.
- [ ] Percentages use available screen after margins. The viewport caps the result if it is smaller than a configured minimum.
- [ ] Resize preserves form/choice/focus state; only the body scrolls when capped; header and actions remain accessible.
- [ ] One consistent action row near the content; no distant vertical Start/Close menu and no filler height by default.
- [ ] No persistent app footer. Details and subdued Help remain in the title/header row; shortcuts live in Help.
- [ ] One clear cyan surface/dialog title, white section headings/ordinary choices, grey secondary text, yellow focus/selection. No duplicated blue title or yellow static “no extra args”.

## Shared search and select

- [ ] One reusable SearchSelect for machines, providers and any other choice field.
- [ ] Search filters by understandable names/secondary labels; selected identity remains stable as results change.
- [ ] Arrow keys move, Enter chooses, Escape cancels the choice without dismissing the parent form. Same behavior for mouse and keyboard.
- [ ] Full option labels, optional meaningful glyph plus text, and unavailable reasons. Do not silently select an unavailable option.
- [ ] Options remain one inline choice region in the existing dialog, not another dialog/page.

## Historical implementation checkpoint — 2026-10-18

The original UI checklist below remains open until current compiled proof and final review. Same product Implementer and independent paired Verifier are active. The revised Client/action design is reviewed ready: native Codex, native background Claude, managed Cursor ACP; saved managed policy/directory/Approver retained. The then-current execution plan is retained privately. The full target was not released for screenshots at that checkpoint; later scoped proof and draft publication are recorded below.

Scoped evidence so far: shared components22/0/1ignored, Sessions398/0/5ignored, public Claude process boundary5/0, compiled PTY19/0 plus capture-support5/0/5ignored. Full post-census CLI sweep488/12/5ignored is not green; two sandbox cases separately passed unchanged2/0 in the real host context, remaining presentation expectations are being repaired. These counts prove their specific boundaries, not final UI/provider completion.

Separate Tailnet lane: actual released.96 reciprocal isolated HTTPS/Origin/board/SQLite proof passed and owned Hosts cleaned up. Correct19741/19742/8443 restart candidates are staged on both machines; no live configuration/process activation. Sunbook's live-config guard is stale and requires a quiet fresh snapshot/check immediately before owner cutover. Small normal-MCP-default source plan is [production ports PR #154](https://github.com/agent-collaboration/agent-collaboration/pull/154), separate from this UI. Source154 now committed/published f0bf9340, current-head run39141807809 has five passing checks, remains draft/unmerged. Causal default test, full CLI486/0 and independent compiled6/0 were read; no production activation or release/install claimed.

## 2026-10-18 owner correction and functional verification

- [ ] Preserve valid managed Stored sessions when extra native-live discovery is unavailable and visibly mark partial coverage. Keep successful-empty distinct, strict source/epoch/cursor/protocol rejection, and captured identity; Root-owned bounded U16 correction and same-reviewer recheck precede this dependent branch.

- [x] Owner withdrew the PermissionDenied/fork/search regression attribution after fixing Sunbook permissions: “it cleared up the other issues the permission caused it.” This is owner-reported environmental resolution, not a product repair or fresh runtime proof. UX defects remain in scope.
- [ ] Arrow navigation must reach Details actions as well as Tab/Shift+Tab; activating Fork/Resume/Copy/Close must produce the correct result without acting on the obscured list. The original defect is repaired at the real component boundary: retained old-handler causal0/1 and current17/0 with exact Fork target. Current compiled terminal/screenshot and broader action proof remain open; do not close from button text alone.
- [ ] Independently exercise all scoped Sessions and Quota workflows through a reusable verification system: keyboard, pointer, search/paste, choice cancellation/Help return, focus/scroll/resize, action results and failure/no-duplicate behavior. Read-only Worker research is commissioned against the existing skill/drivers/tests; paired Verifier retains apparatus ownership. Each requirement needs an explicit pass/fail/unverified result and evidence.
- [ ] Include original screenshots of those actual compiled journeys in the HTML proof gallery; screenshots complement action/result assertions and never replace them. Remote/provider proof remains a separate real-path obligation.

## Sessions

- [ ] Latest direct owner requirement: Claude Code and Cursor must work through agent-sessions TUI by the end of this PR; rectify the previous functional mistake. Reproduce/repair actual provider entry, inventory, Details and action gaps, then prove provider-specific behavior through the real compiled entry. Codex-only screenshots or synthetic provider-label presence cannot close this. Existing source currently makes provider rows read-only and routes --provider claude around the TUI; governing repair is being grounded before dispatch changes.

- [ ] New session uses ONE form, THEN Start. Explicit owner answer: “Show one form, then Start (Recommended)”. Machine and directory visible before launch; no intermediate New-session Details then separate machine page.
- [ ] Preserve machine/source validation and uncertain-action behavior; no silent fallback, retry or cross-machine identity loss.
- [ ] Keep folder scope filters: exact current folder, current checkout/worktree, all linked repository worktrees and all known sessions, as the existing feature supports.
- [ ] Preserve provider/client, source (interactive/subagents/all), runtime-view and updated/created sort controls, session search including full IDs, existing Details and actions.
- [ ] Enter in existing-session lists opens Details. Preserve Resume, Fork, Copy session ID and their existing shortcuts/eligibility.
- [ ] Keep normal clipboard paste behavior and readable Claude/Codex session names/titles.
- [ ] Remove the EMPTY “Launch options” disclosure from the proposal. Existing supplied CLI arguments must still be represented when present; do not remove launch capability.
- [ ] Align all labels and fields in a shared grid. No manually padded strings simulating layout.
- [ ] Reserve the SAME selector gutter for selected and unselected rows. Primary name, provider and metadata begin at the SAME content column; selection must never introduce indentation.
- [ ] Keep full session/title/path/ID available through Details; use available width intentionally in the list.

## Quota

- [ ] Use readable named Quota summaries, spaced middle dots and clear reset counts; use a contextual ↻ only when it improves clarity. The earlier compact glyph-only header proposal is superseded by the later clarity correction; every Quota dialog remains in the common system.

- [ ] Same visual system as Sessions, with original quota information and behavior retained.
- [ ] Account names have priority and remain visible. Do not cap names at13/17 characters while leaving screen width unused; wrap or allocate useful width as needed.
- [ ] Put provider directly below the account name. Use a meaningful stable glyph with a readable provider label, not an unexplained icon alone.
- [ ] Align selection gutter, account name, provider, usage windows and metadata consistently.
- [ ] Preserve the current Account / Status-and-counts / Quota column grouping. Name/provider belong in the first column; status, clients, resets and credits in the middle; reason, Weekly,5h and pace in the last. Withdraw the agent-authored full-width-name/side-by-side-windows mockup. Improve width allocation and wrapping within those groups instead.
- [ ] Use the available horizontal space for weekly/5-hour windows and their labels; avoid an unused right half with clipped names on the left.
- [ ] Make labels succinct and use glyphs where clear, retaining the meaning, units and accessible text.
- Earlier glyph-only header proposal is superseded: retain readable names for summary meanings, complete current counts in Help, distinct unknown/degraded/freshness states and yellow focus/selection. Terminal glyph support remains contextual, not a substitute for understandable text.
- [ ] Format large balances readably, e.g.62.5k in the list and grouped readable values in Details. Never print raw long decimals in the compact summary; preserve the source precision/data.
- [ ] Preserve clients, resets, credit/freshness information, weekly and5-hour windows/reset times, pace, preference/routing reasons and disabled/exclusion information; do not silently remove fields or impose an unrequested row cap.
- [ ] Quota Details, filters, Help, account options and other existing menus use the SAME shared dialog components, one level only.

## Verification and evidence

- [ ] Explicit latest owner requirement: Verification Design plus an HTML proof presentation using presentation-html, retaining original compiled-UI screenshot artifacts. Each screenshot identifies source/build, screen/dialog, viewport, journey/result and limits. Include Quota dialogs.

- [x] Verifier is paired with the Implementer and has the adopted reference and Verification Design. Evidence: paired session settings were created/read back; assignment and upkeep records are retained in the private trace. Actual product observation remains open below.
- [ ] Real compiled iocraft screenshots for BOTH Sessions and Quota at120×36,76×30 and48×24, plus constrained-height/resizing cases. HTML mockups are not real-TUI proof.
- [ ] Select/unselect the same rows and observe unchanged name/provider/metadata x positions; include glyph/Unicode-width and long-name cases.
- [ ] Demonstrate searchable machine/provider/other fields, no-match/unavailable choices and one-dialog depth through real input.
- [ ] Prove no existing feature regresses, especially folder scopes, source/provider/runtime filters, sorting, search, Details and actions.
- [ ] Check content/proportional/clamped sizing, action visibility and focus/scroll preservation through resize.
- [ ] Meaningful product tests, format/lint/type checks, current-head review and required CI before PR readiness. No passing claim from stale source, reconstructed screenshots or a harness that never exercised the product.

## Baseline source defects — fixes require current product proof

- dialog.rs assigns maximum dimensions as actual dimensions and renders actions as a column.
- picker_machine_view.rs bypasses the shared DialogFrame and grows a full-height standalone frame.
- picker_detail_view.rs nests a border and renders static args metadata yellow.
- quota_browse_rendering.rs fixes account names to13/17 characters and status to13/18, even on wide terminals.
- credit_usage_compact_summary prints balance.as_str() without a display formatter, then its row truncates it.
- picker_list_view.rs reserves a two-cell gutter for the title but not the provider/metadata row. The selected/unselected title rows already reserve equal gutters in source. This establishes the metadata defect, not the cause of every shift in the owner's screenshot; real rendered position checks remain required.

## Menu preservation inventory — base ffced1c1

Researcher Worker returned a read-only inventory; Lead inspected the decisive frame, input and sidecar sources. This is source evidence, not runtime proof.

- Sessions: browse/search/paste, machine browse and source-loading state, provider filters, folder scope, runtime view, sort, New session, Details, eligible Resume/Fork/Copy ID, fork destination confirmation, pending source check/cancellation, contextual Help.
- Source Interactive/Subagents/All exists in request/query filtering at this base; the current picker has no visible source-changing control. Keep the request behavior and represent its choice in the shared filters; do not claim an existing visible menu was verified.
- Folder scope includes CLI Cwd/Checkout/Repo/Any. The current picker supports Cwd/Repo/Any and explicitly rejects interactive Checkout; `--checkout --list` remains supported. Preserve all four external semantics. The prototype's fourth folder option is proposed presentation vocabulary, not current interactive proof.
- Quota: browse/focus, provider/disabled filters, Details, Help, account options, Resets/Credits tabs, credit policy Allow/Disallow editor, credit refresh, weekly-floor editor, reset inspection/review/confirmation/result and inventory paging.
- Preserve wide detail sidecars and static output behavior. Menus/editors must use shared chrome and actions at every width; wide rendering must not retain its own footer or keyboard policy.
- Current bypasses: Sessions machine/loading and fork confirmation; Quota wide account-options/floor/reset panels. Existing filters/Details/Help already use DialogFrame but inherit its sizing/action defects.
- [Capability preservation matrix](capability-preservation.md): client-family filtering is distinct from the backend model-provider query. Configured-machine inventories retain their existing scope/query/source/sort guards; searchable controls do not authorize bypassing those guards or silently replacing a draft.


## 2026-10-18 owner correction: required Claude background launch

- [ ] All Claude agents launched by agent-sessions run in background, including literal `agent-sessions --provider claude --new` and TUI Claude Start. Prior foreground behavior must be repaired; prove actual process/background session, not an argv-only claim.
- [ ] Complete private Tailnet configuration on both machines and actual reciprocal debug Router proof with assigned Sidekick. Owner deploys/restarts production manually; do not touch running production Routers.
- [ ] Continue integrated UI, remote-machine and multi-provider proof after network preparation; no forgotten provider acceptance.
- [x] Answer debug API/OpenRouter testing support from current source: upstream overrides exist, but current proxy resolves Router account credentials; Claude replaces client API keys with OAuth bearer. OpenRouter API-key route is not wired. Lead inspected http_sse.rs475–530, claude_edge/forward.rs109–146 and debug_profile_projection.rs45–120; answered in chat. Actual API-key support/testing is not claimed.


## 2026-10-18 retained Chief relay — staging correction

- [ ] Production target is proxy19741, MCP19742, private Tailscale HTTPS8443. Earlier bundle MCP8788 instructions are superseded. Stage all seven named client/profile files and four peer MCP entries on both machines, leaving live configuration and processes unchanged; owner performs cutover. The full source coordination message was read; its identifier is retained privately.
- [x] Actual reciprocal model-free debug MCP/board proof: both TLS directions, Origin403 controls, exact test message persistence through destination Unix and SQLite, normal owned cleanup, original debug profiles/Serve restored and all11protected identities unchanged. Independent report retained in the separate Tailnet workstream; it is not included in this public UI bundle. This is v0.1.96 existing exposure proof on isolated ports46171/46172 and dynamic MCP, not production activation or current redesigned UI/provider proof.
- [ ] Small production MCP-default19742 source PR and matching release/install verification; retain existing intentional test fixtures/debug ports. Production proxy default is already19741. PR154 head f0bf9340 is published with five passing current-head checks and remains draft/unmerged; release/install still open.
- [ ] Readiness/workspace-list tool rename and larger inventory issue: coordinate existing board owner, do not duplicate its change. Current .96 peer tools inventory passed; future board API change compatibility remains to inspect.
- [ ] Report final staged paths/cutover/rollback/order/client reconnect and unproven items to Sun; first direct report held/no active route, not delivered.


## 2026-10-18 first compiled original-image observation

- [x] One Quota Account filters draft survives same live PTY120×36 →48×12 →120×36 with focused Disabled field, title/actions visible; Escape discards draft and normal exit/owned cleanup passes. Three original JPEGs were saved byte-for-byte and independently viewed by Lead. [Gallery and scope](proof.html#compiled-quota-filter-resize). This checks only that case and original-image transport; every broader menu/provider requirement above remains open.


- [x] Unreleased candidate C658 literal public Claude New observed locally: native2.1.296 background handle and full UUID correlated, launch/stop exits0, final stopped, exact own Host/caller/profile cleanup and protected18/18. No prompt submitted. [Scope and remaining provider gates](proof.html#native-claude-new-observed). All-Claude TUI/Resume/Fork/remote acceptance above stays open.

## Current integration blocker and proof frontier

- [ ] Native background jobs must also be discoverable in Sessions. The subsequent real no-prompt run created its job successfully, but one complete own-debug-Host Loaded/Interactive inventory query for the exact UUID returned missing. Root read the actual observation and cleanup. The peer catalogue and native public job list are separate sources. The owner now approved **shared Router catalogue** in the privately retained decision record, preserving both Claude-owned and Router-managed entries and existing interactive sessions; all new Claude launches through agent-sessions must be background. Implementation, discovery, Resume/Fork and remote proof remain open. Public New success above remains valid and narrower.
- Current full Sessions library checkpoint: actual raw log `s2b-inventory-full-lib-second.log` shows 420 passed, 0 failed, 6 ignored, exit 0, before the latest Details changes. Earlier counts above are historical checkpoints, including their retained failures. No final current-target whole-suite pass is claimed.
- Historical checkpoint before the2026-10-19 captures: original Quota proof was three JPEGs from one same-PTY Filters resize journey. Those earlier capture attempts stopped before new product images: high-level browser tab factory timed out; a separate agent-added data-URL control was rejected and withdrawn. A smaller actual HTTP/ttyd fixture observation is being prepared using documented primitives. Remaining menus/grids/provider scenes remain open.

## Direct owner resume and public checkpoint

- [x] Publish the current work as a reviewable draft: [PR #155](https://github.com/agent-collaboration/agent-collaboration/pull/155), immutable first checkpoint `de44ed60b82afcb335c4f0d19e3122e4573148c1`. The public description explicitly lists unfinished gates. This checks publication only.
- [ ] Finish PR #155: catalogue implementation, full current quality, actual Claude/Cursor selected-machine proof on both computers, complete original captures/HTML, independent review and current-head CI. The draft is not ready.
- Direct owner instruction “ok set wakes get to work” supersedes the prior wake-and-stop. Same implementer/verifier are resumed; finite Lead and verifier wakes were saved/read back. A wake firing is not a delivery result.
- Full Sessions library checkpoint now observed in raw `resume-agent-lib-second.log`: **424 passed, 0 failed, 6 ignored, exit 0**. Later changes still need affected checks. Managed-Claude restore must retain its managed API path; the existing Cursor-only guard is an admitted correction, not a reason to convert managed Claude IDs to native IDs.
- Actual browser navigation now reaches the frozen Quota screen at kernel 120×36. The first pending-inspection attempt failed after Ctrl+R left browse visible; no requested image was earned. One separately declared focus/input diagnostic is in progress. Earlier three accepted resize JPEGs stay valid and narrow.

## Sunbook local execution correction

- [ ] Run Sunbook Cursor checks in its logged-in local macOS session with help from Sun / the Station Chiefs. The owner clarified that unlocking Sunbook does not give SSH processes Keychain access. Do not retry Cursor over SSH or infer readiness from an unlocked desktop. The current Sunclaw Station Chief received the request to coordinate local execution; local results and later current-artifact create/load/resume proof remain pending.

## 2026-10-19 actual phone-width baseline

- [x] Correct the observed 32×24 first-frame fragmentation: whole Account/Status/Quota headers, identifiable full account/provider, attributable counts/Credits and intact quota groups. The Lead and runtime Verifier personally opened the actual compiled original in [the phone baseline](proof.html#c7-phone-baseline). Geometry-only checks passed while the screen remained hard to read.
- The one-frame diagnostic passed 1/0 and cleaned up; it is explicitly incomplete and cannot satisfy the normal31-scene/eight-negative journey. Desktop32/40 grids remain separate from actual iPhone operation.
- The bounded next experiment reclaims phone-only decorative padding and the inner table border below 48 columns, retaining the outer border, scrollbar, fixed selector gutter and all three semantic groups. Its deciding test is readability and aligned real frames, not source arithmetic.48+ and static behavior remain protected. No layout success is claimed yet.

### Fresh density comparison — bounded closure

The Lead and same independent UX Verifier personally opened the repaired 32×24 original. Whole headings, fitting account-name words/provider, intact62.5k and attributable client/reset/Credits facts correct the baseline fragmentation. [Before and after](proof.html#c7-phone-comparison) retain unchanged originals. The diagnostic passed 1/0 with independent cleanup, complete=false. Narrow quota 157/0/4 ignored and actual default-frame 9/0, scoped lint/format/size pass; 48×24 mounted frame is unchanged.

- [ ] Complete actual normal 31-scene/eight-negative journeys at all six grids, lower-content/account navigation, constrained height, pending and same-process resize. Tall Quota content and the second account are not established reachable by a single screenshot. Actual iPhone and native/provider/two-machine acceptance remain separate and open.

## 2026-10-19 phone options-title correction

- [ ] Verify the active account-options page remains readable beside Help at32 columns, at body top/bottom and after Tab return. The existing caller now uses “Options · Resets” / “Options · Credits”; the shared header, tab controls and account context remain intact. The mounted regression failed on the old clipped title and passed after the two-label repair; scoped Quota158/0/4 ignored and all-target CLI Clippy passed. A fresh original is still required for visual closure.
- The first full32-column tour failed at a last-voucher wait and exported no images. Its final viewport showed the voucher title after scrolling past its ID. The verifier repaired the tape/checkpoint and rejects wrong voucher numbers/names/suffixes; no ID feature was removed. All31 scenes/eight negative-image checks remain required.
- The final title checks require the complete active header and reject the other tab even when both inactive body labels and the same account tag are present.166 Swift controls passed. These apparatus controls are not application screenshot or full-journey acceptance.
- The earlier full CLI546/0/7 gate precedes this title change; the later158 Quota cases cover the changed presentation. Preserve those scopes and complete final current-head checks before readiness.


### 2026-10-19 current 32-column full export — not accepted

- [x] Lead and runtime Verifier personally opened all31 current unchanged compiled originals from234545; complete selected Claude account name and full wrapped reset success visible.
- [ ] Automatic Normal validation: driver101 at next-account full-name image check; flat OCR interleaves neighboring columns. First browse check passed; later29/eight negatives unrun. Seven-PID/profile cleanup and zero fixture effects recorded.
- [ ] Accepted independent source P2: retain ordinary row status text when it fits without the glyph; Main owns causal boundary/mounted cases and correction.
- [ ] Replace vague Help exit wording with source-grounded user guidance, preserving commit-time protections.
- [ ] Independent cold UX review of all31, other five grids, pending/error states, real resize, Sessions/native providers and actual iPhone remain open.

Current originals and exact result/limits are in proof.html#c7-current-32-review. Original metadata and all hashes are in proof-assets/2026-10-19-c7-normal32-reviewed/evidence.json.


### Independent current32 review disposition

- [x] Separate cold UX Verifier opened all31 originals; complete returned report preserved alongside the unchanged images. NoP0/P1.
- [ ] AcceptedP2: full inventory range/provenance caption must remain readable; natural layout wraps, static output preserved.
- [ ] AcceptedP2: actual captured-name shortening must show ellipsis while preserving full tag; prove full name through existing contextual Help and unchanged draft/Yes/target after return.
- [ ] Evidence gap: show actual post-refresh balance and Credits fact/policy boundary separation; do not infer missing or inaccessible fields from a scroll position. Preserve every previous31/eight obligation while adding fitting original observations.
- [ ] Accepted sourceP2/status and concise accurate Help copy remain in Main's combined correction. Help copy: Exit Quota unless a reset is being applied.
- [x] Credits Edit-alone wrapping classified optionalP3, not a failure: R4 permits horizontal action wrapping and all controls remain. No new action-row design required.

Source/fixture/permanent-test proof, each width, original-image proof, UX judgment, actual device/provider proof and final delivery remain separately bounded.


### 2026-10-20 bounded source closure — fresh images still required

- [x] Independent source Reviewer verified status-labelP2 closed; exact14 current source/test hashes match, no new confirmed defect in the associated caption/identity/Help/synthetic-refresh changes. Root checked decisive source and retained failures.
- [x] Current fullCLI555/0/7ignored across11targets, strictCLIClippy0, globalfmt0, filesize0. Source/commands/results and verbatim independent report are linked at proof.html#c7-code-proof.
- [ ] New38-scene current compiled six-grid originals and independent UX recheck, plus all other existing completion gates. Old31 images do not establish newsource appearance.


### Actual38-scene32-column visual recheck

- [x] Root, runtime Verifier and separate coldUX Verifier each personally opened38/38 original19d0 images. Independent priorP2 range/identity and fullHelp/postrefreshbalance/fact-policy evidence gaps visually closed at32×24; no newP0–P2. Full report and all originals at proof.html#c7-context38-review.
- [ ] Automaticrun remainsfailed:29positivepassed then confirmationcontexttag excluded by measuredsubpixelOCRroundoff; later8/nineNEG unrun.35PID/profilecleanup/zero fixtureeffects recorded. Exactboundedpredicate repair admitted with strongrejectcontrols; no semanticgate deletion.
- [ ] Realistic fixture age: production loader derivesFresh only with known observedtime; normalfixture forcedFresh while retainingunknown age. Root assigned only syntheticage0s correction and strictupdated observation proof. No productionbehaviorchange or realclockclaim.
- [ ] Allothergrids/pending/error/resize/Sessions/providers/actualdevice/finalreviewCI remainrequired. No optionalP3polish/newfeature expansion.


### Current32 age-clarity recheck

- [x] Independent UX Verifier personally opened both changed2c09 originals and verified36 byte-identical prior images. Fresh/checked0s wording ambiguity visually closed; fact/policy spacing intact and no new visible hierarchy defect. Exact originals/report/metadata at proof.html#c7-age-clarity.
- [ ] Automated currentrun remainsfailed after26 positives at native slashed0 recognition; later11/nineNEG unrun. All38 originals and32PID/profilecleanup retained. Only measured field-specific exact-zero interpretation is admitted with strict wrong/stale/geometry negatives; no product change or blanket alias.
- [ ] Allsix complete current runs and remaining required journeys still open. No wholeUI or PR completion claim.


### Completed Normal run at32×24

- [x] Actual first current32 Normal run: wrapper0/driver0,38 scene checks and9 negative checks PASS,64.942s;52 recorded processes/profile absent, zero external/durable fixture effects, original source holds intact. All38 exact pixel hashes carry forward the personally opened independent visual reviews.
- [ ] 48×12 stopped at an offscreen duplicate field-label wait. Choice-only correction now requires actual focused Allow/gutter/active Select; no-match exact field/query/result requirement stays. New supplemental all6 real-component case passes; original2c09 source/build ancestry remains explicit.
- [ ] Remaining4 Normal sizes, pending/error/resize, actualdevice/provider and finalreview/currentCI remain open.

Completed32 evidence and every original scene are linked at proof.html#c7-normal32-pass. This is one-size proof, not wholeUI acceptance.

### 2026-10-20 proof index and remaining short-height capture

- [x] Put the current verdict first and order the 38 unchanged originals by actual workflow. Keep earlier failed attempts in a dated, collapsed history with their existing links and receipts intact. The [proof page](proof.html) separates compiled screenshots, independent visual judgment, source quality and open gates.
- [ ] Latest 48×12 run completed the actual fixture with child/process exit 0 and zero external requests or persisted writes, but its final capture synchronization failed. No composed checkpoint PNGs or Normal receipt were exported. Independent cleanup found all five recorded processes and the profile absent; 2,164 original raw layer pairs remain diagnostic evidence only.
- [ ] Root opened the final raw raster and saw the exact exit marker. Pinned VHS Screen reads old scrollback rows; the component-only cursor-line correction must pass the declared sensitivity and receipt-rejection controls before another capture. This is an apparatus repair, not a product UI defect or a retrospective pass.
- [ ] Proof-page browser rendering remains unverified after the retained file-URL security rejection. The existing v1 library binding is preserved; its cached 1.2.2 guide matches the recorded release hash. Refreshing library documentation returned HTTP 403. Local reference, image-hash, disclosure-balance and diff checks pass; they do not establish browser rendering.

All remaining Normal sizes, pending/error/resize, actual iPhone, provider actions, final review and current-head CI remain required. No broader completion box is checked by this presentation correction.

### Historical 48×12 independent review — finding corrected above

- [x] The separate UX Verifier personally opened all 38 current short-height originals and verified their hashes. Root opened the two initial Options originals, representative dialogs and decisive renderer/action source. The complete report, originals and annotations are in [the current short-height review](proof.html#c7-short-height-review).
- [x] Accepted P2 corrected: the earlier Resets/Credits body was consumed by repeated tabs/divider. Causal regression and fresh current 48×12 originals now establish initial task facts with navigation, identity, title, facts, actions and static output retained. [Independent visual closure and before/after](proof.html#c7-current-normal).
- [x] Keyboard lower-browse reachability repaired at the existing shared Pages caller. Mounted causal checks and the current 42-scene compiled sequence expose compact credits/5h/pace and return without changing account selection. [Current evidence](proof.html#c7-current-normal).
- [ ] Preserve complete intermediate context in the short-height storyboard: full exit rule, inventory/eligibility, selected-credit details, current Help counts and consumption/result guidance. A tail fragment is not proof of the whole statement. These are observation gaps until measured, not claims that product content is missing.
- [ ] The current 48×12 automatic run remains failed: all 38 originals exported, first browse check rejected offscreen `62.5k`; later 37/nine negative checks were unrun. No Normal receipt or gallery was earned. Exact child/fixture zero effects and six-process/profile cleanup remain separately retained.

The prior 32×24 pass remains valid for its declared product and journey. A new product correction needs a new source-bound target; old pixels will not be presented as proof of that correction.


### Repair checkpoint — fresh acceptance remains open

- [x] Causal mounted browse paging regression: old caller 0/1, existing shared Pages policy 1/0; lower compact credits/5h/pace, unchanged identity, PageUp return and account arrows exercised. This is not a fresh compiled screenshot pass.
- [x] Initial Options regression reproduced the first-view defect; the interactive chrome correction passes all six mounted sizes, with static output and existing navigation retained.
- [x] Full Help exit clause measured in one viewport at every required size: at 48×12 one Up from End; other sizes at End. No clause wording removed.
- [x] Full CLI validation after the retained nine-failure run: bounded test/procedure corrections complete; current 560/0/7 ignored, strict lint/format/size and nine copied-default checks pass. The earlier failed aggregate remains failed. [Immutable source and raw logs](proof.html#c7-paging-options-source).
- [ ] Fresh source-bound all-six originals, current independent visual closure, pending/error/resize and the remaining existing gates. The earlier 555-pass result and old pixels retain their earlier source scope.
