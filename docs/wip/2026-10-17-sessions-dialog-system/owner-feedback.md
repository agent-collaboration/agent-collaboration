# Owner feedback — Sessions, Quota and shared TUI components

Canonical checklist for this redesign. Origin: this owner conversation, 2026-10-17. A checked item needs linked evidence. The HTML is a proposal, not product completion. Latest owner corrections override earlier proposed exclusions.

## Current verification checkpoint — 2026-10-19

- The paired Verifier personally opened all 36 final Quota originals: nine scenes each at 120×36, 76×30, 48×24 and 48×12. The Lead opened six changed wide originals and verified the other 30 exactly match previously inspected originals. [Passing compact journey, source/build, actions, visual review and limits](proof.html#quota-current-checkpoint). This is a verified subset, not whole-UI acceptance.
- The fresh compact run passed (1 test, 0 failures): 36 ordered image checks, 16 wrong-image controls, four read-only effect receipts, zero provider requests, unchanged fixture state/secrets and four normal child exits. Independent cleanup found all 64 recorded PIDs and the driver absent, four private profiles and fixture roots absent, and no errors. Small-screen no-match and Shown → Hidden focus are visible. Prior checker failures remain preserved. The final oracle retains strict field associations and measures original label/glyph pixels when OCR omits a chevron.
- Product repair `d7bb5ce8` passed four affected packages: 1,264 passed, 0 failed, 51 ignored; strict Clippy and formatting passed. Commit `f0769c2f` also repairs the private SQLite test cleanup from the prior CI failure. Its focused tests passed 2/2, native integration 4/4, and strict Clippy passed. All five GitHub checks passed for `f0769c2f` in run `39275977331`. This gallery-only successor needs its own current-head checks. These results do not establish the missing UI/provider outcomes.
- The latest actual native Claude New succeeded without a prompt and was stopped. Loaded discovery failed strict timestamp binding; the actual mismatch was not retained, so a product cause is unproven. A later own stopped Stored query matched the UUID, handle, cwd and adjacent vendor timestamps; its whole-run nonpass remains retained.
- Sun parked both machines' existing debug-profile port holders until the owner decides this evening. No signaling, adoption, shared Codex config port write, alternate profile, SSH Cursor/Keychain retry or production change. The instrumented Loaded and Sunbook-local provider observations remain not started.
- The Verifier's until-done 20-minute wake is active and delivery was observed accepted. Root's reminder deliveries were rejected before submission; Root canceled that undeliverable recurrence and reported the gap to Sun. Do not infer continuation from saved reminders. Independent work continues. Complete Sessions/Quota dialogs, lower scrolling, current resize, long names/credits, both-machine provider actions, independent review and current-head CI remain required. PR #155 stays draft and unmerged.

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

- [ ] Latest approved refinement: wide headers show glyph + readable label + count; compact headers keep glyph + count. Use spaces and middle-dot separators between counters, and ↻ for resets. Include every Quota dialog in the common system.

- [ ] Same visual system as Sessions, with original quota information and behavior retained.
- [ ] Account names have priority and remain visible. Do not cap names at13/17 characters while leaving screen width unused; wrap or allocate useful width as needed.
- [ ] Put provider directly below the account name. Use a meaningful stable glyph with a readable provider label, not an unexplained icon alone.
- [ ] Align selection gutter, account name, provider, usage windows and metadata consistently.
- [ ] Preserve the current Account / Status-and-counts / Quota column grouping. Name/provider belong in the first column; status, clients, resets and credits in the middle; reason, Weekly,5h and pace in the last. Withdraw the agent-authored full-width-name/side-by-side-windows mockup. Improve width allocation and wrapping within those groups instead.
- [ ] Use the available horizontal space for weekly/5-hour windows and their labels; avoid an unused right half with clipped names on the left.
- [ ] Make labels succinct and use glyphs where clear, retaining the meaning, units and accessible text.
- [ ] Latest annotated Quota header feedback: use glyphs/icons for repeated account-count/status labels (total, usable, reserve, blocked, excluded) and compact operational counters. Keep meanings in contextual Help, preserve unknown/degraded/freshness distinctions, and keep yellow for selection/focus. The HTML glyph proposal must be checked visually; terminal glyph width/font support still needs compiled proof.
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
