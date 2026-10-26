# Compiled Sessions and Quota journeys

Observe the actual iocraft UI/input/layout through the existing non-publishable
`codex-router-quota-reset-test-harness` fixture entry. Expected outcomes come from
`docs/specs/2026-10-23-quota-dialog-visuals/specification.md` C1–C10 (and its
`verification-design.md` V1–V10) first; `docs/specs/2026-10-17-tui-component-system/`
R1–R14/U1–U14 still govern wherever 2026-10-23 does not supersede presentation.
Never take an expectation from an earlier screenshot, the HTML study or today's output.
See [2026-10-23 visual refinement](#2026-10-23-visual-refinement-c1c10) for what
that version retired and what replaced it.

**Scope:** compiled fixture observation is separate from public entry, live provider effects and delivery acceptance. Use the driver/scene contract below; select applicable exact-source evidence rather than rerunning for a date. Historical results live in `tmp/tui-verification/maintenance-history.md` and their unchanged original receipts. Narrow-width plus short-height desktop proxies satisfy the adopted phone scope; actual iPhone hardware is waived. Same-child resize and pending/error observations remain distinct from static grids. Do not drive a moving build or compete with the Implementer’s build allocation.

## Launch, readiness and evidence

Reusable TypeScript entrypoints live in `scripts/verification/tui/` and run directly with the plan’s declared Node 26.12.1 and locked package dependencies. `evidence-readback.ts` records source and same-child PTY readbacks; `capture-run.ts` drives compact/live routes; `component-capture-run.ts` drives Normal and hard-case Quota; `driver-lifecycle.ts` owns bounded discovery/stop waiting; `public-sessions-run.ts` owns the separately authorized public route. The Rust driver launches the permanent `fixture-child.ts` and `ttyd-adapter.ts` through the same bound Node. Vitest regressions sit beside the entrypoints as `*.test.ts`. Real tapes, lifecycle, Swift checker, native PTY tests and opt-in libtests remain in `crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test*`. These are reusable apparatus/test homes.

Caller-supplied qualified prebuilt artifacts, complete plan/config and an approved execution surface are prerequisites by contract. `--source` is a source declaration, not a complete plan/build/install producer. The existing permanent `run_capture.sh` / `run_provider_filter_capture.sh` compile checker inputs and use nominated local tool roots; inspect their exact environment requirements before use. They install no tools. One-off `run-c7-*`, `package-c7-*` and `publish-c7-*` allocation/packaging/gallery wrappers belong under `tmp`, not in this recipe or permanent script homes. HTML remains Lead-owned; these TypeScript entrypoints emit evidence manifests, not a general HTML generator. Missing producer/tool setup or browser permission is a reported prerequisite gap, not authority to invent a fallback.

Run from repository root with the plan's declared Node 26.12.1 (`TUI_NODE` below
is its `node.path`; the CLI refuses any other Node) and the package's locked
dependencies already present in `scripts/verification/tui/node_modules`. The producer declares
the source HEAD plus file hashes (including untracked product files), exact
compiled test-driver and harness executable paths/hashes, fixtures/configuration,
and build equivalence. Hashes bind artifacts; the producer's build declaration
is not independent proof of compilation provenance. The compiled receipt also
binds its embedded tape/driver/image-verifier sources to current files.

Read `capturePlanSchema` in `scripts/verification/tui/evidence-schema.ts` for the JSON
shape. The `capture_driver` is the prebuilt libtest executable for
`quota_storyboard_capture_test`, not Cargo and not the product executable.
`harness_binary` is the exact harness embedded in that driver at build time.
`node` binds the absolute Node realpath and SHA-256. The driver receives
`QUOTA_STORYBOARD_NODE` and `QUOTA_STORYBOARD_NODE_SHA256`; native receipts
record `node_sha256`.
Supply nominated private VHS/ttyd/image-verifier executables and their hashes,
the existing private temporary directory, dynamic library path and a fresh
nonexistent output directory whose parent exists. These tools must already be
qualified; this recipe installs and builds nothing.

```sh
"$TUI_NODE" scripts/verification/tui/evidence-readback.ts --source .
"$TUI_NODE" scripts/verification/tui/evidence-readback.ts --gaps
"$TUI_NODE" scripts/verification/tui/capture-run.ts "$TUI_PLAN" --check
"$TUI_NODE" scripts/verification/tui/capture-run.ts "$TUI_PLAN"
```

`--check` checks bindings, exact test discovery and the real blank-image negative
control without launching a UI. Apple Vision may be unavailable inside the host's
sandbox even though PNG decoding and language setup succeed. In that case the
checker emits `image-analysis-unavailable`/exit2 and preflight stops. Request the
exact authorized host command through the host's permission mechanism; never
silently widen permissions or reinterpret failure as rejection. One scoped
host-context check has established that the same checker/blank succeeds with
expected missing-marker JSON/exit1 outside that restricted context. Each new
capture still needs qualification in the context that will execute it.
It is a structural check, not product readiness. The actual run selects exactly
`compiled_compact_current_storyboard_exports_both_real_tuis` with `--ignored`,
`--exact` and one test thread. The existing fixture launches the compiled entry
under its own PTY. Each checkpoint waits for an expected screen before saving
the original PNG; the Swift Vision oracle checks actual image markers. A short
paint delay alone proves nothing. Wrong-scene and blank-image controls must fail
their scene oracle. Startup, missing markers, nonzero provider requests,
mutation, wrong viewport, cleanup failure or source/build drift prevent a pass.

The existing Rust harness also exposes the ignored selector
`compiled_compact_current_quota_storyboard_exports_real_tui`. It drives only
CompactQuota through the same tape, renderer and lifecycle at all six declared
grids. Its consumer requires 15 exact ordered Quota checkpoints at 120×36 and
14 elsewhere; omitted, replaced or extra scenes reject before capture.
Select it through `capture-run.ts` with `--quota-only`, for both `--check`
and execution. The mode is mutually exclusive with live CUA, qualifies the Quota
blank-image oracle, requires exactly six Quota receipt sets and each grid’s exact PNG
scene list, and records its actual selector/surface in the manifest. The
default still selects both surfaces. Missing, replaced or extra receipts/scenes,
wrong source/build/tools or failed effect/negative-image/cleanup checks reject.
Apparatus tests and exact ignored-test discovery qualify that validation, not
the screenshots. Failed static runs are retained independently; they do not
qualify a changed target. Each observation binds its fresh producer and source.
Blank/wrong-scene controls remain, including the Quota negative requiring the
Hidden-focus oracle to reject the initial Shown-focus image. Missing, substituted
or reordered auxiliary evidence cannot close the two-step choice proof.

The Sessions-only selector
`compiled_compact_current_sessions_storyboard_exports_real_tui` preserves the
19 non-Details scenes and replaces Gamma Details with measured ordered poses
(`compact_session_details_poses` in `provider_filter_details.rs`, mirrored by
`sessionDetailsPoses` (`scene-contracts.ts`); Root CUA on candidate004,
`tmp/tui-visuals/2026-10-23/lead-session-poses-004.md`). Offsets count Down
inputs from Home: 120×36 fields/path/title at 0; 76×30 fields 0, path 2, title
5; 48×24 fields 0, path 1, title 4; 48×12 identity 0, Endpoint/State 3, path 9,
title 12; 40×24 and 32×24 fields 0, path 4, title 8. Use `--sessions-only` for
`--check` and execution, narrow first. Details is structured
(`render_structured_details`): `Session ID:`, `Endpoint:`, `State:`, `Client:`,
`Machine:`, `Working directory:`, then the `Original title` section holding the
full title, then `Conversation`; below 76 columns the fixed context clips the
title but keeps its tag. `sessionsDetailsPairs` binds the context row (the tag
may be its own right-aligned box), each field on its own label row with stacked
continuation down to the next label or section heading, and the title only
between the `Original title` and `Conversation` headings, inside the dialog
bounded by its heading row and its single `Close …` action row (the `… Esc
Close` hint never bounds it). A truncated value or foreign background never
counts. The strict collector requires 133 originals/19 Details images and 38
wrong images (18 scene controls, one foreign-Codex Details control per Details
image, one short full-ID oracle). Quota requires 85 originals/36 wrong images
(15 scenes at 120×36, 14 elsewhere; six wrong images per grid). BOTH
requires 218 originals/74 negative reads. Derive caps from
`staticCaptureBudgetSeconds` (`capture-selection.ts`); these are maximum operation budgets, not
measured time.
Required source/build/effect/viewport/provider/state and independent cleanup
gates remain. Failed runs preserve their original bytes/caps. Current source
qualification is not current original-image proof or live provider acceptance.

The chooser proves uncommitted labeled fields, not a decorative `Draft:` heading
that may scroll out of a two-row body. Down reveals `Account provider OpenAI`
and `Disabled accounts Shown`; Up restores Provider focus before Help/edit.
Tape waits bind each label/value on one painted terminal row. The Vision checker
retains its original recognition order and resolves these two fields using exact
standalone labels, nearest-right regions and at least half the smaller text-box
height in vertical overlap. It chooses the nearest region before checking its
value; a convenient word in the background cannot replace a wrong field value.
Missing, invalid or ambiguous geometry rejects. Exact contiguous recognized rows
also qualify. `recognizedFieldPairs` is required in private image observations;
the collector checks both actual values. Stale All/Claude, Hidden instead of
Shown, premature close/commit and missing association evidence reject. This
measurement does not infer geometry or visual quality for any other component.

For these declared renderer/theme original PNGs, the checker retains raw OCR and
may measure a dropped Shown/Hidden focus prefix at its exact standalone OCR label
and same-row left gutter. Direct original RGB pixels must establish both yellow
label and yellow glyph-region evidence; bare words, white labels, missing glyphs,
invalid/duplicate/ambiguous geometry and the wrong initial focus reject. No image
is reconstructed or exported by this measurement. `choiceFocusMeasurements` is
required in the strict private observation schema; the collector independently
requires the selected choice. Other palettes/pixel formats are not qualified by
this result. Original image-by-image visual review remains required beyond OCR
and focus sampling. Matcher and protocol controls are apparatus qualification, separate from actual fresh captures.

Derive each fresh packet’s work budget from the current driver and declared grids; never extend a running packet. Normal43+13 uses `240 + 30 × (43 + 13) + 40 = 1960` work seconds per grid, plus the declared preflight and independent cleanup reserves. Compact routes have different scene counts and source-derived budgets. Each image child remains bounded to30seconds. On timeout the TypeScript driver writes only the fixed `capture-stop`; Rust owns exact PTY/browser/image settlement. Timeout remains failure even if the driver exits0. Keep `driver-exit.json`, raw streams, stop error and actual settlement; an unsettled driver is not authorization for broad process signals. Exact libtest discovery is bounded to10seconds.

The wrapper preserves `declared-target.json`, exact command, stdout/stderr and
exit, underlying tape/receipts/PNG/oracle observations, and
`gallery-manifest.json`. Every manifest image names its original path/hash,
screen/dialog, viewport, journey, source/build, scene result, pending visual
inspection and limits. The manifest always says `complete: false` for this
partial route. Inspect the original images yourself; OCR is not geometry,
style, Unicode-width, scroll or action-usability proof. Return the manifest and
original files to the Lead for the proof HTML. The Lead owns HTML and acceptance.

## Journey index and remaining route gaps

The lasting routes share the existing capture ownership, not one common selector: compact `capture-run.ts`, Normal Quota `component-capture-run.ts`, and public Sessions `public-sessions-run.ts`. `evidence-readback.ts --gaps` reports structural gaps, not current accepted run status. No absent fixture seam, renderer or screenshot may be replaced or fabricated by this recipe.

Keep the existing cheap gates beside the image journeys, with their narrower
boundaries explicit. In `quota_reset_pty_test/provider_filter_pty_test.rs`,
`compiled_quota_filters_real_stored_providers_and_disabled_intersections`
drives actual PTY filter/intersection state, unchanged fixture data and no
loopback requests; `compiled_sessions_atomic_paste_searches_literal_id_and_restores_terminal_modes`
checks bracketed paste controls as data, literal ID filtering, no activation and
terminal restoration. These use the compiled fixture, not public provider entry.
`credit_refresh_feedback_test.rs` separately checks actual loopback refresh and
stored feedback/failure facts; it is not a zero-request image journey. Reuse their
current bound runs only after checking scenario, assertions and target identity.
The existing `TerminalDriver::resize` changes a real PTY, but a test that calls
it only before input in a separate child per size does not prove dialog resize
continuity. Use the same live CUA lease/readbacks below for that observation.

| Journey / adopted outcome | Existing drive / required observation | Coverage gap before a declared-target run |
| --- | --- | --- |
| Sessions browse/filters/Help/Details, U2/U3/U5/U9/U10 | Compact tape: browse → native Details/Fork and inline destination → New/machine/no-match → source picker → Ctrl+F inline client/no-match → Help → return/apply Claude → edit/cancel → provider Details at its measured poses → each other filter choice → cancel session. | 19 non-Details scenes plus the measured Details poses are wired; each declared target needs its own current originals. |
| Quota browse/filters/Help/Details, U2/U3/U5/U11 | Compact tape `compact_quota_checkpoints` (`provider_filter_tape.rs`): all → F5 pool-details → pool-details-complete (120×36 only) → pool-details-middle → pool-details-bottom → Esc pool-closed → filters-block-pool → provider-no-match → chooser → help → disabled-choice → hidden-choice → claude → cancelled → F4 details; 15 scenes at 120×36, 14 elsewhere. The four groups (Account / Quota left · reset in / Weekly pace / Extra), glyph meanings and actions require actual image inspection. | Compact Filters, Normal43 and the hard-case journey cover distinct journeys. Reuse only exact-source-bound originals; variants and same-child resize need their own observations. |
| Sessions New/machine/directory/fork/source states, U2/U7/U8/U10 | Open New header action; inspect actual supported client, machine, directory and supplied args; search/cancel machine within one frame; Start once reaches existing substituted preparation; inspect pending/rejected/unknown and fork return. | Draft tape wires native New/machine/no-match/return and Fork/destination. Pending/rejected/unknown and safe Start effect fixtures still awaited. Product tests must assert captured identities/no duplicate launch. Do not perform a live launch. |
| Sessions Details action reachability, U10 and owner navigation correction | Drive compiled public entry: Enter opens Details without launch. Use Left/Right and Tab/Shift+Tab to reach each eligible Resume/Fork/Copy/Close action, observe focus, then Enter and its exact captured-target outcome. Preserve Up/Down body scrolling and modified shortcuts. | Required functional journey, not a label-presence assertion. Current target is unreleased; existing tape visits Details/Fork but does not establish every key/action/effect. Public action/effect binding and original before/after observations remain required. |
| Independent inventory availability, U16/U16-INV-F1 | Through actual local/API, configured HTTP MCP and fixed SSH read boundaries, retain valid Stored rows plus explicit native-live partial notice; successful-empty live is complete. Preserve captured identity and strict invalid/stale guards; keep server private sentinels out of output. | See client-actions.md inventory cases. Real classifier/loader/continuation/publication/component integration and final partial/empty/rejected public-entry images are unobserved; a preclassified fake result or screenshot alone cannot close them. |
| Choice/search/filter preservation, U1/U3/U7/U9 | Exercise typed/pasted mixed-case/secondary labels, no-match/unavailable, stable selection under reorder, pointer choice, one Escape then parent Escape, Help return, all supported typed filters, full-ID paste, stale loader guard. | Draft tape includes all six Sessions fields, client no-match and unavailable Checkout. Nothing yet observed. Product component tests and CUA must establish detailed paste/identity/negative/coordinate behavior. |
| Quota options/editors/reset/paging, U2/U3/U11/U12 | Through existing options: Resets/Credits, credit policy Allow/Disallow/cancel, refresh, weekly floor, reset inventory/review/confirmation/result and paging. Inspect exact decimals/units and return state. | Normal43 uses `component-capture-run.ts` and the fixed normal CLI fixture; compact Filters remains separate. Pending/error variants use the declared live lease. Fixture/effect seams stay with Implementer; never run real account mutation. |
| Three sizes plus constrained 48×12 and resize, U4/U6 | Each changed dialog at 120×36/76×30/48×24, then 48×12 and actual resize with draft/focus/identity retained; header/actions usable; scroll to final body marker; compare selected/unselected columns and CJK. | Static and same-child resize evidence are separate. Require current declared-target originals/readbacks; no static pose alone proves resize continuity. |
| Source/build/current quality and proof gallery, U13/U14 | Original PNGs/manifest plus offered test checks, required product/quality gates and Lead document inspection. | Use current exact-bound retained observations. CI/review/checklist/HTML are separately Lead-owned; images alone cannot close them. |

## Source path and cleanup

### Client-specific agent-sessions coverage

Current admitted S2b mapping and decisive Human-Load/stored-policy cases are in
[client actions](client-actions.md). The current canonical plan is
`tmp/plan-workflows/2026-10-19-tui-component-system-native-catalogue.md`.
The cases below preserve the original inventory distinctions. No target is
released; verify actual source/inventory/action wiring at that declared build.

The owner's later requirement, relayed by the Lead in Router message
`01a14eb3-bf32-7bb0-989a-6b1574ec3ce3`, adds Claude Code and Cursor working
through the **compiled agent-sessions TUI entry** at PR end. Preserve all Codex
cases. The Lead owns the updated action contract; current absent buttons or a
prototype's disabled buttons do not decide what the finished product owes.

| Case | Entry, record and observation | Wrong behavior the offered proof must catch |
| --- | --- | --- |
| CLIENT-CODEX | Real agent-sessions entry with local/hosted Codex records; preserved filters, Details, New/Resume/Fork, captured source/model/arguments and existing failure rules. | Adding provider support routes a native record to the provider runner, loses its source or drops the existing model/passthrough contract. |
| CLIENT-CLAUDE-HOSTED | Real entry → existing inventory → `HostedProvider` summary plus uniquely qualified Claude Code endpoint → TUI → adopted client action/effect boundary. Retain complete service/endpoint/session identity. | A Claude label/filter or copied ID passes while the intended action remains inaccessible or invokes Codex. |
| CLIENT-CLAUDE-INTERACTIVE | Separately exercise `ClaudeCodeInteractive` origin/status/working directory and the adopted eligible actions. Do not equate it with a Router-hosted provider merely because both display Claude Code. | A hosted-only fixture never admits terminal-origin sessions, or resumes/copies the wrong opaque identity or directory. |
| CLIENT-CURSOR-HOSTED | Real entry with actual Cursor provider-origin record and qualified endpoint evidence, search/Details and adopted client action/effect boundary. | Cursor glyph/label appears but no usable action exists, or an equal Claude/Codex ID is selected instead. |
| CLIENT-QUALIFICATION-NEGATIVE | Equal opaque IDs across clients/endpoints; label resembling Claude without typed endpoint evidence; contradictory/multiple channels; unavailable/changed source; late inventory/reorder while Details/action is open. | Classification by display label, index-based retargeting, silent fallback, duplicate effect or unsupported action admitted. |
| CLIENT-REAL-ENTRY | Pin an actual compiled `agent-sessions` executable as well as its source/configuration and provider inventories. Run from its public argument entry under real PTY, observe client-specific actions and exact captured identity, then retain original screenshots and effect-boundary evidence. | The feature-only picker fixture bypasses parser/inventory/dispatcher and is mistaken for public executable proof. |

The current public chain is `src/bin/agent-sessions.rs` →
`session_environment::run_arguments` → `session_command_dispatch` →
`TerminalSessionsPicker` → real component. The current provider inventory loader
in `session_commands/picker_runtime_inventory.rs` reads external-provider
endpoints, requests Stored/Any/All and projects typed summaries. The storyboard
fixture already includes hosted Claude, interactive Claude, hosted Cursor,
equal IDs and a deliberately unclassified Claude-lookalike, but calls the picker
directly with no loader and is **not public-entry or live-provider proof**.

Do not reuse the pre-implementation source-gap snapshot as a current capability
matrix: U16 changes are in progress. At the released target, inspect actual
Stored/live inventory projection, origin-aware identity, New Client choices,
Details capability actions, public dispatch and selected-machine adapters.
Keep absent or unobserved behavior open; source declarations and injected rows
do not prove public discovery, usable provider actions or vendor compatibility.
A backend model-provider string such as `cursor` is not typed client selection.

### Details keys must reach effects

Start with a known source-qualified record and keep its full identity captured
while another row/inventory refresh changes background focus. Through the real
compiled public entry, ordinary Enter must open Details without an effect.
Walk the action row independently with Left/Right and with Tab/Shift+Tab; observe
each focus transition before activation rather than relying on a fixed key count
or a visible action label. Activate Resume, Fork, Copy session ID and Close in
separate owned runs when an action exits the app. Resume must reach the eligible
captured client/source once; Fork must open the captured destination confirmation
and Cancel must have no effect; Copy must request the exact captured ID once and
stay in Details; Close must return to the unchanged browse selection. Unsupported
provider Fork retains its actual reason and produces no effect.

Repeat at all required grids, including constrained height. Up/Down and paging
must still reveal the long body without silently moving action focus; Left/Right
must navigate the action row. Check Ctrl+Enter/Ctrl+Shift+Enter separately so
shortcut success cannot hide an unreachable action. Repeat after same-PTY resize
with draft/focus/target retained. Preserve original focus/action/result images,
actual key sequence and exact action/effect readback; images of button text alone
cannot pass. A substituted copier can establish requested bytes/count at the
real component port, with that limit explicit; it is not OS clipboard delivery.
The existing fixture marker only names the terminal picker outcome and session
ID; it does not establish public parser/inventory/dispatcher or vendor execution.

The test challenge must inspect actual scenario/assertion/entry boundaries for
all cases above. Existing inventory/read-only and component filter tests can
support those narrower claims; they cannot establish a usable provider action.
Model-free qualified service/command stand-ins may prove the real dispatch and
captured target where the approved design permits, with their integration limits
named. Actual provider runtime proof remains unobserved and gated: no live
provider/model input or funding change is currently authorized. Return missing
public-entry fixtures, compiled artifact, action contract or runtime grant to
the Lead; product seams/tests remain with the Implementer.

Actual resize uses CUA on a live owned ttyd URL, rather than mid-tape dimension
settings (VHS skips those after startup). No raw CDP, Playwright controller or
browser-console mutation is authorized. The existing VHS static route remains
separate. CUA's supported browser viewport capability sends real browser resize;
ttyd connects that to the existing PTY's TIOCSWINSZ. This route is implemented
as partial apparatus. One short Quota Filters resize has been observed on a
declared compiled fixture; other dialogs/sizes remain open. Leave U4 open.

### Live CUA lease and actual size readback

After the producer releases the target, use a new plan/output directory for each
surface and run:

```sh
"$TUI_NODE" scripts/verification/tui/capture-run.ts "$TUI_PLAN" --cua-surface sessions
```

For the separately declared all-menu Quota component fixture, set the plan's
`harness_binary` to the exact CLI **library test binary**, then use
`--cua-surface quota --cua-quota-fixture normal` (or the exact declared scenario).
This reuses the same owned ttyd/PTY lease; it adds no renderer or UI controller.
Preflight must discover exactly its ignored fixture test as well as the capture
driver test. Current Rust lease changes compile and the artifact receiver has
real HTTP/filesystem checks. One normal Quota live lease has been observed after
its serialized target/build handoff; new runs still require their own release
and allocation. The existing VHS static route remains
separate. CUA originals are JPEG, retained as JPEG without pixel conversion.

CUA plans must declare `cua_artifact_scenes`: a nonempty list of unique scene
names, at most 128, each at most 96 lowercase ASCII letters/digits/hyphens and
starting/ending with a letter or digit. Static plans may leave it empty. The
wrapper passes these exact names to the owned lease; an older driver without
`artifact_export` readiness is rejected. Declare every image before launch,
including separate before/after resize scenes. Do not add names after seeing
results or reuse a filename from an earlier run.

### Retain original CUA bytes through the owned export form

The same lease starts a bounded loopback-only artifact receiver. Its readiness
record contains `artifact_export.base_url`, fixed output directory, allowed
scenes, maximum bytes, JPEG MIME and embedded receiver-source hash. The adapter
checks these against the plan and source. The export form is a separate owned
tab, not another ttyd connection. Opening the terminal URL twice could launch
another PTY; never use that as the export tab.

Use only the documented CUA screenshot/AX APIs and in-memory byte operations:

1. Inspect the actual terminal scene and matching process/grid readback. Take
   its original screenshot with `emit: false`, retaining the returned JPEG
   `Uint8Array` in the same CUA runtime. Record its byte length, lossless standard
   base64 with `btoa`, and SHA256 with `crypto.subtle.digest` before submission.
   Do not transform pixels, convert formats or use filesystem/browser-console
   APIs in CUA.
2. Open a separate owned tab at exactly `artifact_export.base_url + scene`.
   Read its fresh AX state. Through documented `setValue`, fill **Original byte
   length** and **Original JPEG base64**, then click **Save original**.
3. Read the saved page. Require its byte length and SHA256 to equal the original
   buffer values. Ordinary filesystem tools then independently verify the fixed
   `original-images/<scene>.jpg` and `<scene>.receipt.json`. Preserve the image's
   source/build, screen/dialog, actual viewport, journey/keys/effect, result and
   limits beside it. A byte receipt alone supplies none of those UI judgments.
4. Close only the export tab. Repeat for each declared scene while the original
   product and receiver are alive. Save all images before normal product quit;
   the receiver shuts down with the lease. Keep the original terminal tab and
   same child identity across resize.

The form uses a random per-lease nonce, fixed predeclared filenames, create-new
0600 files under a new 0700 directory and an 8 MiB image limit. It rejects
duplicate/unknown fields, unsafe scenes, wrong nonce, invalid base64/length and
overwrites. It rereads the saved bytes and checks exact equality plus base64
round trip before writing the hash receipt. JPEG sentinels are only a format
check; neither the receiver nor its synthetic protocol-byte tests judge pixels.

The wrapper requires `artifact_sink_stopped: true`, the exact declared artifact
count, matching receipts and retained hashes after normal exit. Its lifecycle
summary lists `original_artifacts` with result
`original-bytes-retained-ui-judgment-separate`. Receiver HTTP/filesystem tests and
in-memory base64/SHA primitives pass. **Actual CUA screenshot-to-file transfer is
qualified for the first normal Quota lease:** all three original buffer lengths/
SHA256 values matched the saved JPEGs and encoding receipts; the receiver stopped
with the owned lease. Keep the same equality checks in each later run. That result
does not establish another scene's UI behavior. No undocumented API, alternate
controller or reconstructed image substitutes for the actual interaction.

The component fixture runs exactly the named ignored libtest with an inherited
real PTY and a closed scenario, isolated environment and no vendor process.
For pending cases, first capture/inspect the actual pending scene, then write
exact `settle\n` bytes to `cua-quota/fixture-settlement`. Do not create it early.
Keep the same PID/birth/executable/source across resize. The fixture settles
only its stand-in effect; this does not demonstrate a real account operation.
Its completion must create `cua-quota/fixture-finished` with matching scenario,
released flag and zero external requests/persisted writes. The adapter rejects
a missing/mismatched receipt even when the libtest process exits zero, recording
the actual process exit separately and failing the outer lifecycle gate.
The producer's durable-receipt extension and this lease were validated together
for the first released normal scenario: `released=false`, zero external requests/
persisted writes and normal exit. Pending settlement and other scenarios still
need their own observation; controlled-child apparatus tests do not close them.

This leases one fixture for up to 600 seconds, starts the existing fixed child
adapter through loopback-only ttyd, and writes
`cua-sessions/cua-ready.json`. Use `--cua-surface quota` for Quota after finishing
Sessions. No VHS browser or tape runs in this mode. The same libtest selector
is only the fixture host: a green lifecycle is not a screenshot or UI pass.
The ready record explicitly means **listener ready, UI not observed**; the
product process starts when the first CUA tab connects. Open exactly that URL
once in an owned CUA tab; a second websocket client could create another PTY.

Read the browser's CUA viewport capability documentation. Set the viewport in
pixels through its supported `set`, inspect the resulting terminal and use this
read-only check to bind each actual grid to the live child:

```sh
"$TUI_NODE" scripts/verification/tui/evidence-readback.ts --cua-plan "$TUI_PLAN" --surface sessions --columns 120 --rows 36 --record "$TUI_OUTPUT/cua-before.json"
"$TUI_NODE" scripts/verification/tui/evidence-readback.ts --cua-plan "$TUI_PLAN" --surface sessions --columns 48 --rows 12 --baseline "$TUI_OUTPUT/cua-before.json" --record "$TUI_OUTPUT/cua-after.json"
```

`TUI_OUTPUT` is exactly the plan's output. Pixel dimensions are calibrated from
the rendered terminal, not asserted to equal grid dimensions. The check reads
the wrapper's kernel terminal-size file, verifies source/binaries/UID/executable,
and requires the same child PID/start/group/executable across the baseline and
later readbacks. It performs no browser action. A mismatch stops the claim;
never rewrite viewport metadata to fit a screenshot. Preserve query/draft/focus
and selected identity before resize, inspect them after each of the required
sizes, and preserve each original CUA image with the matching readback. CUA may
return original JPEG rather than PNG; retain its actual bytes and format rather
than converting it and calling that an original PNG. Exact image retention and
the all-menu drive remain distinct: retention passed the first scoped Quota pass,
while all-menu coverage remains open. Use the owned
export form above; retaining bytes in CUA memory alone is not a saved artifact.

Quit the product normally (cancel choices/forms first, then Sessions Ctrl+D or
Quota q), close only its owned CUA tab, and reset the temporary browser viewport.
The fixture host verifies exit, zero provider requests, unchanged fixture state
and exact owned-process cleanup. Its `cua-receipt.json` and wrapper summary are
**lifecycle-only**; original images/observations are separate. `cua-stop` inside
that live output requests early cleanup but returns failure, never a UI pass.
If the browser cannot reach/resize/capture the target, return the exact live URL,
process/viewport readbacks and steps to the Lead rather than adding a controller.

The capture test in
`crates/codex-router-quota-reset-test-harness/tests/quota_storyboard_capture_test.rs`
calls `provider_filter_capture.rs`; its tape inputs are in
`provider_filter_tape.rs`. `scripts/verification/tui/fixture-child.ts` starts the fixture-bound compiled
binary. Its feature-only `src/main.rs` dispatches Sessions through
`run_sessions_picker_test_harness`, and Quota through
`run_quota_reset_test_harness` → the existing Quota command/component.
Fixtures replace records/external effects, not iocraft/input/layout. The capture
owner records exact child/process identities; receipt checks require child exit,
no provider requests, unchanged state/secrets and stopped owned processes. This
is compiled fixture-PTY proof, not Ghostty, personal data, live remote discovery,
provider launch or account mutation proof.

Use the existing Rust capture lifecycle's exact recorded-process cleanup. Never
kill by executable name, remove evidence directories or restart production.
Retain a failed run's output and inspect its process settlement before any new
run. Wrapper interruption is not a cleanup receipt. A new attempt uses a fresh
output directory and a newly confirmed target. Confirm evidence survives cleanup.

Apparatus checks (not product observations):

```sh
pnpm --dir scripts/verification/tui typecheck
pnpm --dir scripts/verification/tui lint
pnpm --dir scripts/verification/tui format:check
pnpm --dir scripts/verification/tui test
```


## Existing component normal tour and public variant upkeep

The confirmation correction retains the same title/choice/target spatial band and
allows only1e-6 normalized lower-edge tolerance (under0.001 pixel in these originals). Qualify touching/roundoff, real-overlap, below-choice, above-title and background negatives before trusting the spatial gate.

Current Sessions compact readiness is bound to its actual inline field labels:
Client, Machine and Destination, or the named filter field. Expanded choices own
Select; closed parents retain Apply, Start or Fork. No-match observations require
the exact active-field query `zznomatchzz`. The uncommitted Client Codex checkpoint
requires spatially associated `client codex`, including the narrow stacked field;
a decorative Draft summary or background session name cannot qualify it. The
collector requires that exact associated pair too. The 19 non-Details scenes and measured Details pose contract above are
retained. Matcher controls must reject wrong-field/query/parent-action and stale/background/nearest-wrong-value evidence; source qualification is not a compiled Sessions observation.

Only the four normal floor checkpoints normalize this observed heading's joined
or separated whitespace. They require an exact recognized heading line; an
incidental `Weekly floor` warning cannot substitute. All other field/value,
header-region, forbidden-marker, effect and cleanup checks remain. Keep controls for joined/separated, wrong/missing headings and background-warning rejection; apparatus qualification is not a UI pass.

`"$TUI_NODE" scripts/verification/tui/component-capture-run.ts PLAN.json` selects only `compiled_normal_quota_component_dialogs_export_real_tui` and `presentation::quota::quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture` under fixed `normal`. `ComponentPlan` adds the exact nominated CLI library-test executable and unique supported grids to `CapturePlan`; no normal product executable substitutes. `componentStates` in `scripts/verification/tui/scene-contracts.ts` and `NORMAL_CHECKPOINTS` in `quota_component_tape.rs` own the same43 ordered scenes. They cover browse/lower-credit/5h/pace/return, Details/Help, floor draft/cancel/save/reopen, initial task content, reset inventory/paging, Credits62,500 then refreshed63,000/freshness/0s observation/policy boundary, policy search/no-match/choice/cancel/save, reset No/Yes/contextual full-name Help/unchanged return, result/consumption/guidance/return. A final tail alone never proves an unseen intermediate semantic field. The reset-result-consumption checkpoint requires the complete provider result and consumed-credit sentence together; the mounted six-grid proof measures two Down events at48x12 and zero at the other five grids before that pose. The former42-scene tour missed this short intermediate fact, so it cannot satisfy that coverage claim.

The strict Normal consumer requires all43 ordered scene originals and exact PNG census/hashes, declared grids, source/tool/driver/tape/adapter identity, actual normal child completion and all13 wrong-image controls. `collectComponentImages` (`component-collection.ts`) owns the exact positive order and rejection pairs. Missing, substituted, repeated, reordered or extra evidence fails. Saved floor requires associated `Current 1%`, not unsaved draft `Floor 1%`; refreshed63,000 must reject initial62,500. A short result may expose its full body in one image, so top need not fail bottom. Zero external requests/persisted writes, durable scenario receipt, successful wrapper/child and independently settled owned resources remain required. Fixture refresh does not prove provider freshness.

VHS exports final PNGs after tape execution/teardown, so pending images cannot be
validated before release within that static route. Use the EXISTING component CUA
lease/original JPEG sink for pending and actual resize; a matching finite target/
action packet is required. Preserve exact `settle\n` release only after the actual
pending original is retained/validated, then same-PTY result and normal cleanup.
Do not release from text-only readiness, bypass phase guards, substitute internal
frames, or cancel an unreleased pending scenario and call its adapter successful.
The eleven existing scenario names remain the only nominated fixture variants.

Compile all current `verify_screen_*.swift` inputs through the existing capture/control shell owners. Bind every source and the resulting checker; there is no reduced-input fallback. Runtime receipt fields retain their existing source/binary schema; a control-only source does not invent a runtime field.

Public Sessions source upkeep uses the adjacent `public_sessions_capture.rs` and
`public-sessions-run.ts` over the SAME existing CUA/PTY/artifact-sink route. Fixed
argv is only the bound public executable; approved cwd, inherited normal HOME and
CODEX_HOME (including absence), explicit debug root and expected Host service/epoch
are required. The existing owned-runtime qualification checks actual process/
binary/health/manifest before effects. The fixed child rechecks hash/directory/
manifest before its retained Node child; public wrapper UID/birth/command identity is
qualified before signalling. Rust never signals the public child by a nonexistent
fixture capability. Its strict lifecycle is distinct from fixture zero-effect
receipts and refers to helper-owned actual effect evidence; UI exit never proves
native/provider job settlement. Actual public runtime remains blocked until Root
supplies a legitimately owned free Host and allowed-effects packet. Parked Hosts
are not adopted, stopped, signalled or reconfigured by this upkeep.


Normal-tour feasibility corrections retain failed runs independently: the actual
VHS parser rejects Home/End, so the tape uses supported repeated Up/Down and the
runner validates the exact generated tape before UI startup. Edit opens the policy
choice immediately: Select is the expanded action, Save is collapsed. Choice Escape
before original/cancelled/saved field checks preserves the editor. The normal
reset No/Enter path cancels back to Browse-owned options Close/Inspect; reinspection
and actual Review/Confirming/Yes acknowledgements precede confirmation. Result
Escape waits for settled Close/Inspect before the second Escape. Returned browse
rejects active-dialog/result markers and must reject the wrong result image as its
fourth negative. Two additional wrong-top images must reject the Help/Details
bottom scenes; all six negative receipts are required by the strict collector.
These are source/feasibility corrections until a fresh exact
pilot completes with originals and cleanup; no failed run becomes a pass.

Contextual Help legitimately lists keyboard guidance: only component HelpTop/Bottom exempt browse’s hidden-shortcut ban. Other scene footer guards remain. Bottom observations require actual final Details note and Help forced-exit instruction; saved top images must fail those bottom scenes. Preserve required fresh-runtime wrong-image controls.

Browse name waits require the actual selected-row gutter and every fixture name
token in order, allowing bounded wrapping only back into the name column. This
replaces an agent-added single-line-name assumption. Missing/wrong tokens,
unselected/background names, wrong continuation columns and excessive gaps reject.
This is readiness qualification, not proof of rendered identity or selection;
personally inspect each emitted original and retain its source/build/grid/limits.


### Adopted UX and desktop phone-proxy scope

The adopted correction brief is
`docs/wip/2026-10-17-sessions-dialog-system/ux-correction-brief.md`.
Root's latest declared static matrix is120×36,76×30,48×24,48×12,40×24 and32×24.
The existing normal component plan and both compact Rust selectors now support
those six grids; the compact collector requires every declared grid. Normal
component requests reject unknown/duplicate dimensions before any UI starts.
Phone-width receipts must retain the actual requested width; a48-column receipt
cannot satisfy32/40. Kernel width alone does not prove content fits: check the
actual composed cell bounds and every original for clipping/overflow.

Semantic upkeep remains incomplete until the C7 caller changes are concrete.
Require active field + query + no-match result together, unavailable Select with
no commit, captured decision identity and irreversible risk inside the dialog,
one pointer/keyboard No/Yes group, named header counts in the header, and full
current census values in contextual Help with draft/query/focus preserved on
return. Keep short-body actions adjacent to content and long-body final markers
reachable; actual resize must preserve identity, draft and focus. Main owns the
real composed cell/input tests; runtime V checks their sensitivity and opens each
fresh original; the cold-reader UX Verifier independently assesses it. Existing
wrong-scene, source/build, effect and cleanup gates stay required. Retained known-
bad originals qualify new semantic checks where mechanical judgment applies;
page-wide incidental words cannot substitute for header/field associations.

Phone proof scope is narrow-width plus short-height desktop PTY proxies. Actual iPhone hardware is explicitly waived by the owner. Keep requested physical columns/rows, actual raster and each pose’s limits; a48-column receipt cannot satisfy32/40, and static grids do not establish same-child resize.


C7 confirmation source cutover follows the actual single pointer-capable choice
group: "Confirm reset", captured alpha-tag context, "1 credit. Cannot undo.",
"Reset quota?" and exactly one No/Yes pair. Tag/risk must occur below the actual
dialog title and above its choice, so background context cannot qualify. The
new controls have no decorative [No]/[Yes] text. The same real journey still
requires No+Enter cancellation, then Right+Enter and the reset result. Focus
colors and pointer/disabled semantics require Main's actual composed cell/input
tests plus personal and cold-reader inspection of both fresh originals. The
semantic check alone makes no selected-color claim. Current compiled originals must establish the adopted concise label and result hierarchy; earlier wording does not decide the expectation.

Compact Quota source expectations likewise follow the actual field labels:
the retained `Account provider` plus nested `Search: zznomatchzz▏` (C7 end caret),
with `No matching choices` and unavailable `Select`,
then `Disabled accounts` on the initial choice and same-process Shown/Hidden
focus. Generic `Search:` or collapsed `Apply` cannot qualify that expanded field.
The exact scene order is `compact_quota_checkpoints` in `provider_filter_tape.rs`
(15 scenes at 120×36, 14 elsewhere) with its negatives/effect/cleanup duties.
Interactive Details uses labelled Status/Reason facts beside its fixed identity;
readiness and image checks no longer require the removed duplicate Selected
account heading. Static/sidecar output is outside that interactive expectation.


C7 compact account context follows the adopted caller-owned prefix + captured tag
slot. Options/Credits identity checkpoints require alpha-tag rather than an
impossible full-width label at32/40 columns. Full names remain required in their
browse/Details observations and accessible through contextual Help. Full captured
name Help/scroll and return still need their offered real component tests and
actual original observation; neither the compact tag nor a truncated prefix alone
earns that obligation. No name requirement is silently marked complete.


The2026-10-19 first-phone diagnostic is explicitly separate from Normal. Its
fixed `compiled_quota_phone_first_frame_diagnostic_exports_original` selector
runs only the initial48×12 normal-fixture browse, captures one original, then
sends graceful Ctrl+C and waits for actual child exit0. VHS defers PNG export
until its tape finishes; a later expected wait failure does **not** preserve an
earlier Screenshot. Retain the failed no-image attempt and never replay it.
The corrected diagnostic writes only `first-frame-diagnostic-receipt.json`, kind
`compiled-quota-first-frame-diagnostic`, complete=false; the Normal collector
rejects it, and Normal keeps all43 scenes/13 negatives. A separate source-
bound target and finite execution/custody handoff remain required.

Normal tape readiness follows the selected full name through actual wrapping.
After the density cutover, continuation at the phone Account origin is exactly
outer border + two gutter cells; retain the wider observed prefixes as well.
VHS Wait can right-pad short rows with hundreds of blank cells. Readiness may
ignore that trailing whitespace, while wrong name, missing fragment, other
column, absent gutter and appended name suffix still reject. Returned browse
uses the same complete selected-name wait rather than a single-line OpenAI
Research phrase. These lexical waits are synchronization, not optical identity
proof; original-image and all Normal negative/effect/cleanup gates stay required.

Root subsequently adopted the shorter caller captions Options·Resets and
Options·Credits so the full active page fits the existing phone header budget.
Tape waits, image checkpoints and gallery labels use those complete meanings.
Both inactive tab labels can be visible in the body: separate Options/Resets or
Options/Credits word presence is insufficient. The checker requires the complete
active header on one OCR line with known whitespace/·/• normalization (and its
adjacent Help action); cross-tab controls keep both body labels and alpha-tag.
Old long titles and fuzzy Re/Cr fail. Compile and qualify the current matching checker/driver against those header controls before observing a declared target.

The capture owner also declares VHS's supported Output diagnostic/frames/
destination before recording. On natural command errors, pinned VHS attempts
to move its original numbered text/cursor PNG layers there before cleanup.
The fresh owned parent must share the recorder temporary filesystem; the frames
target stays absent before launch. The collector rejects missing, unpaired,
malformed or symlinked layers and records hashes, paired dimensions and explicit
composed=false/exact_failure_instant=false/checkpoint_evidence=false limits.
PNG envelopes are checked in Rust; a runtime control must independently decode
the retained pair. The layers are unmodified canvas rasters, never composited
images or replacements for the declared normal PNGs. Forced termination, ignored
VHS rename errors or failure before recording can still leave no retained layers.

The private fixed compiled_quota_natural_error_retains_original_raster_layers
control reaches first browse then the known impossible screen wait through the
existing component/OwnedCapture owner. It may run once only after a matching
current-source driver release and explicit custody handoff, within50s preflight,
120s work and90s cleanup (260s whole). It must fail at that exact natural VHS
wait, retain validated/hashed original text-cursor pairs, and independently settle
owned processes. Its diagnostic receipt is complete=false and effects unobserved;
it cannot satisfy Normal43/13-negative, effects, exact failure-time or UX gates.
The retention source preparation and controls remain unqualified until their
recorded compile/runtime outcomes exist; old failed frames cannot be recovered.

Root admitted exact selected-name association at the existing image owner. Native
Vision word boxes constrain the full ordered name to the actual Account column;
the original selected yellow text and first-row gutter must agree. Missing,
malformed, ambiguous, neighboring-column or other-row geometry rejects, as do
wrong/missing/reordered/suffixed name words. Preserve causal controls rejecting wrong/missing/reordered/suffixed words and background/other-row geometry. Qualification never rewrites image bytes or a prior failed aggregate.

Help-bottom requires the full meaning “Exit Quota unless a reset is being applied.” Vague or unconditional copy rejects; controls must cover that distinction.

The43-scene contract retains contextual Help entry, full ordered captured identity/tag, unchanged Yes selection on return, initial62,500 and distinct post-refresh63,000/Freshness/Observation/policy-boundary observations. No old screenshot or failed aggregate is promoted by a later checker audit. Derive counts from current `COMPONENT_STATES`/`NORMAL_CHECKPOINTS`, not historical31/38-scene packets.

Normal work budget is1960seconds per declared grid (240VHS +30×(43+13) image guards +40 cleanup allowance);50preflight and90independent cleanup yield2100seconds for one disjoint job. A combined packet derives its own matching reserve shape. Never borrow or extend an old packet’s cap. Native Help body association excludes background names and malformed/ambiguous bounds; Help return preserves the nondefault Yes choice.


The current coherent Normal fixture declares its initial and refreshed observation
age as `0s`; the post-refresh view must show `Observation checked 0s ago` and
reject the retained older `not checked` original. Initial 62,500 and refreshed
63,000 remain distinct observations. This fixture statement is not a live-clock
or provider claim, and other unknown-age scenarios retain their coverage.

Only contextual Help entry and full-identity checkpoints use supported native
Japanese plus English OCR with language correction disabled; every other view
keeps English OCR. The unique title and Close bound the spatial body, with
distinct row centers and overlap limits excluding chrome and background text.
The whole Account field ends at Current quota when visible or at Close when
that next heading is offscreen. Its comparison preserves every ordered CJK and
Latin character and the complete tag; existing case normalization and Unicode
whitespace removal apply only to the name. The entire tag value is case-exact;
only width-equivalent bracket decoration normalization applies to its slots. Missing, extra, reordered, adjacent, malformed or ambiguous content rejects.


The post-refresh Observation checkpoint uses the original title/tag observations
and native word boxes to bind the nearest value to the unique Observation label
inside Options · Credits and above Close. Only that measured field accepts the
exact native readings `checked 0s ago` and `checked øs ago` of the font's slashed
zero. Raw OCR lines remain unchanged. Other ages, units, extra or missing words,
old unknown values, duplicate chrome/labels/tags, background/right-column values,
nearest wrong values and malformed bounds must reject. This is a scoped OCR
interpretation, not a global character replacement or a fixture wording change.

The expanded Credit usage choice may scroll its duplicate `Use credits:` label
out of the short body. That checkpoint instead requires the unique Credit usage
title and captured tag, the exact Allow row inside the active body with native
yellow text and gutter, and visible Cancel/usable Select actions. Ambiguous,
background, clipped, unavailable or wrongly focused controls reject. The no-match
checkpoint still requires its field label, exact query and No matching choices;
its separate FocusReveal path exposes them at the short grid. Reuse a prior capture only when its actual image/source/fixture dependencies still apply; a checker supplement is separate evidence.


Compact Quota Filters Help first acknowledges its title, then scrolls to the
complete field-control entry below Current quota: Down3 at48×24, Down8 at48×12,
Down6 at40×24 and Down5 at32×24;120×36 and76×30 need none. The permanent mounted
`compact_filter_help_controls_are_reachable_and_preserve_draft` witness measures
those poses and checks exact draft/focus return. The screenshot still requires
the contextual controls, and closing Help must preserve the uncommitted filters.

### Component completion after terminal scrollback

The component wrapper prints final validated `QUOTA_STORYBOARD_CHILD_EXIT` on a fresh cursor line without trailing newline. Component Normal/first-frame tapes use anchored `Wait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/`. This is synchronization only: real child/wrapper exit0, exact durable fixture receipt, zero external requests/persisted writes, original images and independent owned cleanup remain mandatory. Early/forged markers cannot replace these. Other surfaces retain their own existing framing.

The fixed48×12 first-frame diagnostic exports one original with `complete:false`, never a Normal receipt. Permanent real-PTY controls concurrently drain forty lines and check final-line framing plus missing receipt, nonzero-child and forged-marker rejection. Keep diagnostic and full Normal outcomes separate.

### Normal43 lower-content and native-reader contract

The versioned Normal contract is43-positive/13-negative. Preserve every declared semantic property and all four lower-content originals. Reuse `browse-credit-window` for62.5k credit and `browse-pace-heading` for BOTH5h/99% and Weekly pace; these are separate images on the same Alpha account. At48×12, the credit and burn-unavailable poses send256 PageDowns, acknowledge the complete bottom group (`Extra`, `Credits unknown`, `2 resets`), then reverse5 PageUps for credit or6 for burn-unavailable. Only the heading pose pages forward2 from the top. Every lower pose returns with PageUp256. Credit uses PageDown1 at48×24 and32×24; every other pose stays at Page0. The permanent mounted witness `normal_short_browse_groups_retain_alpha_and_return_exactly` in `crates/codex-router-cli/src/presentation/quota/quota_normal_browse_pose_test.rs` proves the complete short-grid groups and exact returns against an independent tall reference. Literal Home/End remain unsupported by pinnedVHS; the permanent mounted observer must prove exact poses and same initial-frame/identity return. A foreign selectedBeta must fail BOTH lower-credit and short-window/heading even if unselectedAlpha facts remain visible. Unreadable/ambiguous visible selectors fail closed. Offscreen names/gutters carry same-process journey continuity limits; no CJK alias or flat-marker-only attribution is permitted. Prior receipts retain their original contract.

Initial Resets/Credits screenshots must occur before any manual scroll and expose a meaningful actual task fact. Root accepts Checking/Live eligibility or actual voucher data; voucher-first is a synthetic fixture witness, not a new layout requirement. Initial Credits policy/fact is distinct from later exact balance. Main's current product removes only redundant interactive tabs/divider and restores browse Pages, with static rendering, navigation, all facts and pending assurance preserved. Caption/balance/freshness/observation/policy positions must be bound to that new compiled fixture; old2c09 offsets do not apply. At48×12 Help End requires one Up to capture the full exit guard; other grids need none.

Each Normal grid budgets50preflight +1960work +90independent cleanup =2100seconds. The13 negatives include foreignBeta against both lower-credit and short-window/heading plus the pace wrong-result image. Current source/control qualification, exact TypeScript/Rust census consumers, pinned six-grid VHS parsing and fresh product/tool bindings precede execution; source preparation is not capture success.


The current43-scene native reader uses one accurate request with language correction disabled and no custom words. Only the three new lower-fact checkpoints and contextual Help use `[en-US, ja-JP]` with automatic script detection. Weekly-floor draft uses `[ja-JP, en-US]` without automatic detection to read its literal native numeric percent box. Other checkpoints retain English. Floor1% must be the unique active yellow value associated with Floor inside the title/tag/action body; guidance1%, arrows, background/duplicate values and wrong states reject.

Selected account extraction derives the fixed two-cell physical selector column from the unique Account header and requires one actual yellow glyph row, yellow name words and the exact ordered full CJK/Latin identity. Adjacent native glyph boxes may overlap by less than half the smaller height; distinct row centers must descend by at least half the larger height, with at most one row-height edge gap. Same-row words retain ordered centers separated by at least half the larger word width. For overlapping same-row boxes, the original decoded intersection must contain zero selected-yellow ink through the existing pixel predicate; no fixed pixel-overlap allowance applies. Ink overlap, merged or duplicated words reject. The first selected native word alone may cover the source-defined two-cell selector slot when actual yellow text/glyph prove it; other words retain the Account-column boundary. Missing, merged, duplicated, reordered, foreign and malformed data reject. Native word ingestion ignores only nil/empty/whitespace-only enumeration payloads before semantic regions; it preserves every nonblank substring and rejects malformed nonblank boxes. Raw recognizedLines and native evidence remain unchanged.


Root's source-grounded field interpretation separates the captured full Account NAME and exact entire tag VALUE from the one-character presentation decorations on each side (`quota_dialog_context.rs::full_label`). The same measured Account field, exact name/value length, every identifier character and field endpoint remain mandatory. Only literal ASCII/full-width bracket slots are admitted automatically; the entire tag value remains case-exact, while labels and the name retain their existing normalization; no global character replacement, identifier alias, substring/prefix/suffix search, raw-OCR rewriting or mode/candidate change applies. Missing/extra/internal/ambiguous/other-field or same-name/different-tag data must reject. Automation does not establish literal bracket APPEARANCE: every affected original must be personally inspected for actual complete bracketed tag and readability, and its gallery/personal evidence must state that limitation. The failed literal-OCR40 audit and its raw native candidates remain failed/unchanged; a corrected semantic field audit is separate evidence, never a rewrite of a prior run.


### 2026-10-23 visual refinement (C1–C10)

The Quota browse is now four groups — Account / Quota left · reset in / Weekly
pace / Extra — wide only from 85 columns (120×36); the other five grids fold and
repeat each heading per account. Census moved to F5 Pool details; Quota browse
and Session Details footers name their actual bindings. Each superseded check was
retired with a replacement; old-layout originals are wrong-image evidence only:

| Retired (old layout) | Replacement (owner) |
| --- | --- |
| Selected name bound by unique Account + `Status` header | Account heading plus its header-row neighbour, or full width when folded; a name's own `Account` word is rejected by column-gap guards (`verify_screen_identity.swift`) |
| 32×24 `Unknown` pace on a 350×484 Account/Status/Quota raster | `selectedWeeklyPaceMatches` at every grid: wide Weekly pace column inside the selected block (ends at the two-blank-row separator), folded own Weekly pace heading above Extra, short-grid continuity only when no account start is visible (`verify_screen_pace.swift`); tape waits `burn unavailable` everywhere |
| Census counts in the browse header (`requiredQuotaHeaderCounts`) | `requiredPoolCensus` bound inside the Pool details dialog body (a second recognition of its title-to-Close rows; a split count binds only to its label's adjacent box); every browse scene rejects `Label N` census in the header rows above the group headings, row by row (`verify_screen_visual.swift`) |
| Shortcut-text ban on Quota browse and Session Details | Required actual hints (`F4 Details`, `F3 Filters`, `F5 Pool`, `F1 Help`, `Ctrl+R Options`; Details `focus`/`activate`) plus a ban on the HTML sample letters and `Account actions`; Details also rejects any Enter-as-fixed-Resume label. Other dialogs and Sessions browse keep the ban |
| `Search: zznomatchzz` | Tape terminal-text waits require exactly `zznomatchzz▏`; the image field pair allows at most one caret-like OCR glyph (`verify_screen_sessions.swift`) |
| `62.5k` credit, `99%…5h…Weekly pace` order | `62.5k credits`; pace heading in either wide (header above) or folded order |

Compact Quota adds `pool-details` (F5, top), `pool-details-complete` (all ten
pairs incl. zeros; 120×36 only, where the whole body fits),
`pool-details-middle` and `pool-details-bottom`, `pool-closed` (Esc) and
`filters-block-pool` (F5 while Account filters owns input). Wrong images add
Pool details vs unchanged browse and closed vs still-open Pool details.

The top, middle and bottom Pool poses each require a census subset that fits
the smallest body (48×12 shows four rows; mounted frames in
`tmp/tui-visuals/2026-10-23/implementer/pool-six-grid-body-proof.log`): top
Accounts/Usable/Reserve, middle Blocked/Pool unknown/Excluded (hard adds
Fresh), bottom the rest. Their union is all ten rows at every grid; the hard
bottom and the 120×36 complete pose also require `Credentials locked` as a row
inside the modal. The middle pose is offset 0 on tall grids and three (compact)
or four (hard) Downs at 48×12, derived from the mounted body; the native
waits and oracles must validate these derived offsets on a declared run.

The separate hard-case journey (`compiled_hard_case_quota_component_exports_real_tui`;
`component-capture-run.ts` plan `"scenario": "visual-hard-cases"`) drives the
component fixture `visual-hard-cases`: Alpha's known windows, Beta's stale
`75% · week · —`, `No data · 5h · —` (no meter glyph on that row), `125.5 credits`
with `Credits stale`, the visible `Credentials locked` error, the complete hard
census in Pool details, Esc return to Beta and Beta's Account details with its
captured `[beta-tag]`. 120×36 uses nine scenes (`HARD_CASE_CHECKPOINTS`);
76×30 and the 24-row grids eight (`HARD_CASE_PARTIAL_CHECKPOINTS`, no complete
census pose). At 48×12 the top pose fits only the name's first row,
so the full name never fits one frame: fourteen named poses
(`HARD_CASE_SHORT_CHECKPOINTS`) prove identity as the same-child composite of
the selected name prefix (top pose), its selected continuation one page down
(`selectedNameContinuationMatches`: top body row below the toolbar, painted
selected, directly over its own client line, no Account heading or other
account; Vision may split it into word boxes on that row) and the captured
Details tag. Every Alpha-selected browse pose reads its CJK prefix with a
zh-Hans-first word pass used only for the selected name; a misread modal identity
tag is re-observed by an en-US pass over its own row; with every heading scrolled away the header-less
selector gutter is anchored on the row-start staleness label. Alpha week is at PageDown 2 and 5h at 3;
Beta week at 2, stale/no-data at 3, credits at 6 (candidate003 live CUA). A
distinct `pool-closed-top` keeps the shared tall `pool-closed` full-name check.
Window lines bind by terminal row, Quota-left column and account block
(`missingWindowLines`).

Capture tools: build VHS 0.12.1 with native F1/F4/F5 from
`quota_storyboard_capture_test/vhs_f4/README.md` into a private root beside the
official ttyd 1.7.7_14 / libwebsockets 5.0.0_2 bottles. All Swift checker sources
(`verify_screen_*.swift`, including `verify_screen_pace.swift` and the 2026-10-23
`verify_screen_visual.swift`, `verify_screen_visual_controls.swift` and
`verify_screen_native_field_controls.swift` and `verify_screen_reading_repairs.swift`) compile into one checker whose
`--matching-controls` must report zero failures before a run.

Not established by these images: exact RGB tones and fills (mounted/ANSI tests
plus personal inspection of lossless PNGs), NO_COLOR (fresh compiled PTY test),
clipboard bytes (mounted substituted writer), Pool census rows between the first
and last at 48×12 (taller grids and mounted tests), Vision readings of `·`, `—`
and footer F-keys, the hard 48×12 name continuations, the measured Sessions
Details poses and the derived 48×12 Pool middle offsets until a declared run
qualifies them.
