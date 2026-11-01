# Compiled Sessions and Quota journeys

Observe the actual iocraft UI through the non-publishable
`codex-router-quota-reset-test-harness`, using a Rust-owned PTY. The oracle is the
cell grid of `@wterm/ghostty` 0.5.7 (libghostty). Original PNGs render the same
bytes with `@wterm/dom` 0.5.7 in headless Chrome over DevTools. These images do not
establish physical terminal or keyboard behavior.

Expected outcomes come from the adopted four documents in
`docs/specs/2026-10-29-tui-component-system/` and the UI PR plan in
`docs/wip/handoffs/2026-10-30-tui-component-system-ui-pr-plan.md`.
Prior Quota presentation obligations in
`docs/specs/2026-10-23-quota-dialog-visuals/` C1–C10 remain except explicit later
owner decisions. Earlier screenshots and current output do not define correctness.

## Declare and qualify the target

Use an allocated isolated checkout with no competing build, Node 26.12.1 and
locked dependencies. Sign every directly executed binary with the repository's
`scripts/cargo_debug_signing_runner.sh`. Never install debug binaries or replace
production processes. Preserve the allocation's disk floor.

```sh
pnpm --dir scripts/verification/tui install --frozen-lockfile
node scripts/verification/tui/terminal/terminal-qualification.ts QUALIFICATION.json
node scripts/verification/tui/evidence/evidence-readback.ts --source REPOSITORY
node scripts/verification/tui/capture/capture-run.ts PLAN.json --check
node scripts/verification/tui/capture/capture-run.ts PLAN.json
```

Read `gridCapturePlanSchema` in `scripts/verification/tui/capture/capture-run.ts`.
A plan names `repository`, exact `sourceHead`, `driver` and `product`
`{path, sha256}` bindings, fresh nonexistent `output`, `journey`, and `keys`
(`legacy` or `kitty`). Normal/hard require `component`; optional `chrome` names the
qualified Chrome executable for original PNGs. Paths are producer supplied.
Hashes bind artifacts; the producer must establish compilation provenance and
that the driver embeds the nominated harness build.

The driver is the prebuilt `quota_storyboard_capture_test` libtest, with exact
entry `compiled_grid_terminal_observation`. Normal/hard require the `agent-router-quota-tui` library
libtest containing
`quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture`.
A CLI libtest without that entry rejects before startup.

Before requesting any native window, invoke the exact driver entry and arguments
with the case's surface, mode, artifact and fixture environment, adding
`TUI_GRID_CHECK_ONLY=1`. Require exit 0 and `TUI_GRID CHECKED <surface> <mode>
120 36` before requesting the window. The driver starts at 120×36; narrower cases
then use the journey's acknowledged same-child resize. This admission path stops
before opening the PTY or spawning the fixture. Keep its stdout/stderr, exact
binary hash, admitted configuration and evidence that no child PID files or
product receipts were written. Wrong surfaces, modes and artifact hashes must
refuse. Hash/signature and libtest-entry checks alone do not establish admission.
The separately declared `showcase` surface validates its example launch through
the fixed wrapper's `--check-only` path, including its path/hash, working directory
and owned receipt directory. It does not observe the showcase.

For the showcase, use only the canonical lane's runner, `PtyGridDriver`, readers
and Rust driver at one declared apparatus commit. The separate showcase checkout
supplies its example artifact and source binding only; never execute its old TS
apparatus. `showcase/prepare-plan.ts --example-repository` records separate
`source` (apparatus) and `exampleSource` bindings. The runner refuses a different
apparatus checkout and checks both source bindings before and after observation.
After argument admission and before requesting a native window, run
`node scripts/verification/tui/showcase/qualify-transport.ts PLAN.json`.
It starts the actual TS/Rust/wrapper pair for every declared mode, requires the
exact `READY 120 36 STREAM` contract and the wrapper's own ready bytes, then
stops it without spawning the example. Require exit 0, absent driver/wrapper
processes, no example PID or product receipts and no cleanup-error marker.
Keep `transport-receipt.json` and each mode's raw protocol/lifecycle output.
This qualifies transport only; the product cases still require their own window.

`--check` verifies head, signed artifacts, exact entries and emulator qualification;
it does not observe a product. Qualification covers named/24-bit colours,
bold/dim, 📂/✉️/界 widths/graphemes, overlay-left−1 gutter writes, right-edge
spacers/wrap metadata, kitty query/reply and resize, recording versions and WASM
hash. Missing fixture, tool or host permission is a prerequisite gap. Never
switch renderers or fabricate bytes to fill it.

## Journey index

`capture-run.ts` drives the literal data in `journeys/` through
`journeys/journey-catalogue.ts`. One file per screen under `journeys/screens/`
contains shared actions and explicit size-specific data. The canonical grid list
in `journeys/journey-definition.ts` is `120x36`, `76x30`, `48x12`, `32x24`;
paired-marker cases and authored flows use that same list. A wired definition is not a passing observation.

| Prefix | Entry and deciding observations |
| --- | --- |
| `sessions-` | Provider-filter fixture: browse, Details/Fork/New, machine selection/no-match, each filter choice, contextual Help, applied Claude filter, cancellation, full Details identity/path/title poses. |
| `quota-` | Mixed-account fixture: browse, Pool census top/middle/bottom and complete ten rows at 120×36, close, Filters blocking Pool, provider query, Help, Disabled/Hidden selection, apply/cancel and Account details. |
| `normal-` | Exact Normal component fixture: 43 checkpoints covering selected Alpha/Beta, lower credit/5h/pace, Details/Help, floor draft/cancel/save, initial task facts, inventory/paging, Credits and policy/no-match/cancel/save, refresh, No/Yes/contextual Help return, result/consumption/guidance and return. |
| `hard-` | Exact `visual-hard-cases` fixture: known Alpha windows, stale selected Beta weekly/Extra, Pool census/locked-credentials notice, close and Beta Details. Short height has distinct identity/window poses. |
| component-system baseline | `baseline-journeys.ts`: main Esc, Help Esc wording, New focus/Down, Account-details spacing, existing-overlay input isolation and covered drawing. Future menu/toolbar/dropdown entries are absent at base. Wide-under-dialog requires the selectable product fixture. |
| public Sessions | `public-sessions-run.ts`: separately qualified real executable, same grid/original sink, preserved normal home/cwd/Host/effect/job contract below. |
| S9 authored flows | [Authored journeys and native bindings](s9-new-entry-journeys.md), `journeys/flows/flow-catalogue.ts --check`: 31 B flows, 212 grid/protocol variants. `authored-flow-run.ts` drives admitted fixture adapters; unsupported bindings fail before startup. Catalogue checks launch no product and report zero product passes. |
| component showcase | `showcase/run.ts PLAN.json [--check]`: SC01–SC27 through the lane's canonical transport and readers against the separately bound ordinary example. Defaults are four grids and both modes, with independent Quit, glyph, clipping and lifecycle cases. Run `showcase/qualify-transport.ts` before a window request. A valid plan or handshake is not a scenario pass. |

SC25's anchored-overlay reseed is draft until driven on a newly declared example.
Its private `seed-wide-glyph` action (Ctrl+Alt+S) places ordinary `UiText` at
the published overlay edge minus one, clamped to zero. This is fixture setup,
not proof: close the overlay and independently require the actual width-two
glyph on every covered row in the original PTY grid. Only then judge the saved
covered frame's left border and gutter. Reopen and independently qualify that
actual footprint against the retained uncovered parent grid too.
Repeat that parent qualification after the same-overlay resize and after
restoring the viewport; a seed from another viewport is invalid. A predicted
centered width never qualifies an anchored dropdown.

The supplemental lasting entry `input-journeys.ts PLAN.json [--check]` extends
`baselinePlanSchema` with explicit `preparationProof`. It observes New's five
base Tab targets and reverse wrap, Start through Tab/Enter and a click, one safe
terminal outcome, main Enter/filter/Esc behavior, kitty modified Enter and legacy
Details fallbacks, Ctrl+C on main/Help, and Quota Q. Its own coverage limits stay
in the result: a UUID-only terminal marker is not full source-qualified identity;
Quota exit is not its committing guard. Main toolbar/focus-model targets absent
at base are recorded as absent, never a pass for full R3.

For S0 use `{kind:"pending-s6"}`: Start's terminal outcome does not prove
preparation dispatch. S6's agreed strict receipt is
`{callCount:1,request:{client,machine,directory,runMode}}`, with stable client/
machine identifiers and runMode `foreground` or `background`. The recorded
step names a fresh relative JSON filename and the independently declared expected
request. It rejects missing receipts, extra fields, any count other than one, and
any changed committed field. That historical S0 preparer placeholder is superseded for local/default New by
the [returned-outcome reader](input-consumption-readback.md); only configured
New retains a counted preparer. Never accept
`pending-s6` as dispatch proof.

`wide-glyph-baseline.ts PLAN.json [--check]` extends `baselinePlanSchema` with
optional Chrome. It selects the test-only `wide-glyph-left-border` variant and
checks New/Details at120×36 in both key modes, requiring all three glyphs at
overlay-left−1 on covered title rows. It measures left-border loss; row8 is a
blank separator, so it claims no corner seed. Geometry or entry failure is a gap.

Each transport burst is one write through the existing Rust driver. Legacy leaves
the kitty query unanswered; kitty mode drains all emulator replies and encodes
keys accordingly. Only layer A can prove deterministic same-update event batching.

Usable readiness is an actual completed synchronized frame containing Sessions or
Quota status. A process/listener marker is insufficient. Steps acknowledge frames
or exact active-dialog fields before deciding a checkpoint. An unreachable entry
is never a product red. Same-child resize requires identity/content continuity;
static images or different children do not establish it.

## Assertions and sensitivity

`grid-markers-*.ts` retain 138 literal scene contracts. `grid-scene-matcher.ts`,
`grid-identity-checks.ts` and `grid-dialog-checks.ts` bind active-dialog fields,
unique field associations, counts, selected identities, labelled Pool census,
coherent window rows, selected Floor value, contextual Account name/full tag,
and confirmation identity/risk/unique choices. Background text cannot supply a
missing active field. CJK/grapheme continuations use cells. Tags preserve case
and exact identifier length; names retain the adopted whitespace/case normalization.

`grid-control-checks.ts CAPTURE_DIRECTORY JOURNEY [FRESH_OUTPUT.json]` rechecks each positive before
requiring rejection of the retained wrong-scene and empty-core controls. Each
positive also rejects blank. A supplemental audit writes a fresh output; it never
overwrites an earlier control receipt. Preserve all wrong-scene pairs: top cannot prove
bottom, Cancelled cannot prove Saved, foreign selected Beta cannot prove Alpha's
lower facts. Oracle sensitivity is separate from S5's seeded wrong-build proof.

Explicit baseline decisions supersede stale prose checks:

- Unavailable Weekly pace is the selected account's ⏳ cell and its continuation
  under Weekly pace, as pinned by `quota_normal_browse_pose_test.rs`.
- Compact Beta browse shows weekly 75% and `week stale`, omitting unknown-window
  placeholders. Beta Details separately requires weekly 75%, reset unknown and
  no invented 7d; 5h No data with no percentage. This is adopted #161 behavior
  from `6388b9739`. The short-height unknown-window port still needs its Details
  pose before it earns four-grid coverage.

## Evidence and cleanup

Each run binds current head and source hashes, including apparatus/untracked
product inputs, and checks artifact/source drift. Each checkpoint keeps `.ansi`,
`.grid.json`, `.txt`, `.observation.json`; original `.png`/`.original.json` bind
bytes, source/build, viewport, checkpoint, renderer, Chrome, Node and WASM. Compare
browser text with the emulator, then inspect originals for readable complete
identity/tag decoration, clipping and full viewport. Cells establish content/style;
originals establish rendered appearance. Neither is physical-terminal proof.

Success requires every checkpoint/control, real child exit0, fixture effect
checks, unchanged source/artifacts and owned cleanup. Raw stdout/stderr/commands
survive failures. Timeout, missing receipt, external write, mutation or unsettled
cleanup prevents success. Fixture runs permit sealed GETs only, unchanged
state/secrets, and component receipts with zero external requests/persisted writes.

Use the retained driver's `stop` command and exact owned processes, never process
names or another checkout's children. Chrome uses isolated profiles and its own
process group. Shutdown first awaits browser exit, stops its remaining owned
group and awaits inherited-pipe closure. A controlled final-error subprocess
regression requires natural exit 1 and absent browser/helper processes; this
is apparatus coverage, not a product observation. Independently check owned
processes after native runs. Delete profiles only under exact-path authority, with no open handles and
all evidence outside them. Keep failed runs unchanged; later passes are separate.

## Public entry, effects and jobs

Layer F / CLIENT-REAL-ENTRY uses the public route at S9. Never substitute the
synthetic fixture or its zero-effect claim. S0 ports and tests the sink without
launching the public runtime.

`public-sessions-run.ts PLAN.json --check` uses `publicPlanSchema`: source plus
Node/driver/public binary bindings; approved absolute cwd; debug Router root;
expected Host service/epoch; existing runtime plan and bound effect evidence;
ordered original scenes; key mode, Chrome binding and declarative key/text/resize/
checkpoint actions. Each checkpoint pattern is readiness, not Host/provider effect
proof. Read-only qualification still needs an already authorized live Host.

Preserve actual Router process identity/executable hash, manifest Host/version/
proxy endpoint, health version and effect-artifact hash. The fixed child rechecks
exact public binary, no extra argv, approved cwd, manifest service/epoch, inherited
normal HOME/CODEX_HOME including absence, and explicit debug root. It gets no
fixture capability or substituted home. Public termination preserves the existing
wrapper UID/birth/command guard and signals only that retained wrapper; the Node
wrapper reaps its own retained child. Fixture immediate-kill behavior stays as is.
Keep native bystander/termination and kernel-resize tests. Lifecycle evidence
binds source/Node/binary/Host/effects and keeps
`job_settlement: separate-runtime-helper-proof-required`.

UI exit/originals never establish native/provider job settlement. Execution needs
a legitimately owned free Host, its allowed-effects packet, and independent
provider/client/job readbacks. Never adopt, signal, stop or reconfigure a parked
Host. Any needed Host/effect change returns to the Lead before editing.

## Coverage limits

The S0 pilot drives four existing journeys at 120×36 in legacy and kitty modes.
Report their exact receipt/source dependencies. Four-grid and same-child product
resize, pending/error variants, wide-under-dialog, complete new focus/navigation,
S5 wrong builds, real public-entry/provider/client effects and physical terminals
remain separately required or explicitly unverified. Full rollout observation is
S9 at its declared head. Source preparation and emulator fixtures are not those
observations.

The old-original comparison was not performed: old tools/originals were missing
or unqualified; the Lead explicitly accepted the gap. Never substitute a synthetic
or reconstructed comparison. Run results and receipts belong under the task's
`tmp/tui-verification/`, not in this recipe.
