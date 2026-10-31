# Compiled terminal verification

The Rust `TerminalDriver` owns each PTY. TypeScript sends its next input, feeds
actual output bytes to `@wterm/ghostty` (libghostty), and checks the cells. All three
wterm packages are pinned at 0.5.7. The separate original-image sink renders those
bytes with `@wterm/dom` in headless Chrome through DevTools. No reconstructed cell
image counts as an original. A headless image is not physical-terminal evidence.

Use Node 26.12.1 and pnpm 12.14.0, from the repository root:

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

`gridCapturePlanSchema` in `capture-run.ts` declares the repository, exact source
head, signed driver/product/component artifact paths and SHA-256s, fresh output,
journey name, `legacy` or `kitty` keys, and optional Chrome executable. The driver
is the prebuilt `quota_storyboard_capture_test` libtest, not Cargo. Component
journeys require the exact fixture-bearing Quota library test executable; the
runner checks its test entry before launching it. Source files and artifact hashes
are recorded and checked for drift. Build provenance remains the producer's duty.

`journey-catalogue.ts` names 24 literal journeys: Sessions, Quota, Normal43 and
hard cases at 120×36, 76×30, 48×24, 48×12, 40×24 and 32×24. Data ports exist for all
six grids; the S0 pilot observed only 120×36. A definition is not a passing receipt.
`baseline-journeys.ts` separately records the component-system's pre-change
failures, passing isolation cases, absent future entries, and unobserved fixtures.

Every checkpoint keeps raw ANSI, JSON cells, text and assertion results. Each
original keeps its byte/image hashes, renderer/WASM/Chrome identities, source,
build, viewport and checkpoint. Each journey rejects the actual empty-core and
retained wrong-scene controls before writing a success receipt. An entry timeout
is an apparatus/feasibility failure, never a product defect.

`public-sessions-run.ts PLAN.json --check` qualifies the separately authorized
public route without launching its TUI. Its Host/process/health/effect checks,
normal HOME/CODEX_HOME, exact public argv/cwd and job-settlement boundary remain
separate from fixture zero-effect receipts. Running that route requires its own
allowed-effects packet. S0 does not launch it.

Native wrapper tests still exercise the real PTY, kernel resizing, retained-child
cleanup and durable fixture receipts. They require nominated canonical
`QUOTA_STORYBOARD_NODE` and `QUOTA_STORYBOARD_NODE_SHA256` matching Node 26.12.1.
Their stand-ins prove wrapper contracts, not product appearance or real provider
jobs. Never install debug product binaries or replace production processes.

See the canonical repository recipe:
`.agents/skills/verify-codex-router/references/tui-journeys.md`.
