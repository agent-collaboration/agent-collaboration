# Compiled terminal verification

The Rust `TerminalDriver` owns each PTY. TypeScript sends its next input, feeds
actual output bytes to `@wterm/ghostty` (libghostty), and checks the cells. All three
wterm packages are pinned at 0.5.7. The separate original-image sink renders those
bytes with `@wterm/dom` in headless Chrome through DevTools. No reconstructed cell
image counts as an original. A headless image is not physical-terminal evidence.

Use a privately retained Node 26.13.0 runtime and pnpm 12.14.0, from the repository root. Copy the complete Node runtime (including libnode) into an owned toolchain directory before declaring its canonical executable path and SHA-256; a mutable Homebrew symlink is not the retained runtime. Keep its dependency inventory with the declaration and re-qualify after any runtime dependency change. Historical 26.12.1 receipts remain evidence for their original runs only:

```sh
pnpm --dir scripts/verification/tui install --frozen-lockfile
pnpm --dir scripts/verification/tui typecheck
pnpm --dir scripts/verification/tui lint
pnpm --dir scripts/verification/tui format:check
pnpm --dir scripts/verification/tui test
node scripts/verification/tui/terminal/terminal-qualification.ts OUTPUT.json
node scripts/verification/tui/capture/capture-run.ts PLAN.json --check
node scripts/verification/tui/capture/capture-run.ts PLAN.json
```

`gridCapturePlanSchema` in `capture/capture-run.ts` declares the repository, exact source
head, signed driver/product/component artifact paths and SHA-256s, fresh output,
journey name, `legacy` or `kitty` keys, and optional Chrome executable. The driver
uses a prebuilt signed libtest, not Cargo. Quota and component routes use the
`e2e` entry `quota_storyboard::compiled_grid_terminal_observation`. All S8 picker, quota and showcase routes use that namespaced entry; public
Sessions uses `quota_storyboard::compiled_public_sessions_grid_observation`.
The S9 cutover retires the historical flat-entry driver selection. Discovery and
launch share `ptyGridDriverEntry`; no alternate entry is tried. Component
journeys require the exact fixture-bearing Quota library test executable; the
runner checks its test entry before launching it. Source files and artifact hashes
are recorded and checked for drift. Build provenance remains the producer's duty.

`journeys/screens/` contains one data definition each for Sessions, Quota,
Normal43 and hard cases. `journeys/journey-catalogue.ts` expands them over the
canonical `journeyGridSizes` in `journeys/journey-definition.ts`: 120×36, 76×30,
48×12 and 32×24. The 16 screen/grid variants retain their checkpoint IDs, action
bursts and finish sequence. Size-specific scrolling is data inside each screen
file. A definition is not a passing receipt.
`capture/baseline-journeys.ts` separately records the component-system's pre-change
failures, passing isolation cases, absent future entries, and unobserved fixtures.

Every checkpoint keeps raw ANSI, JSON cells, text and assertion results. Each
original keeps its byte/image hashes, renderer/WASM/Chrome identities, source,
build, viewport and checkpoint. Each journey rejects the actual empty-core and
retained wrong-scene controls before writing a success receipt. An entry timeout
is an apparatus/feasibility failure, never a product defect.

`sessions/public-sessions-run.ts PLAN.json --check` qualifies the separately authorized
public route without launching its TUI. Its Host/process/health/effect checks,
normal HOME/CODEX_HOME, exact public argv/cwd and job-settlement boundary remain
separate from fixture zero-effect receipts. Running that route requires its own
allowed-effects packet. S0 does not launch it.

Native wrapper tests still exercise the real PTY, kernel resizing, retained-child
cleanup and durable fixture receipts. They require nominated canonical
`QUOTA_STORYBOARD_NODE` and `QUOTA_STORYBOARD_NODE_SHA256` matching Node 26.13.0.
Their stand-ins prove wrapper contracts, not product appearance or real provider
jobs. Never install debug product binaries or replace production processes.

See the canonical repository recipe:
`.agents/skills/verify-codex-router/references/tui-journeys.md`.
