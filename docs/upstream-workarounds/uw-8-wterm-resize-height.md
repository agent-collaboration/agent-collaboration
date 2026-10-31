# UW-8: wterm keeps the initial fixed element height after resize

**Status:** in place, 2026-10-31
**Dependency:** `@wterm/dom` 0.5.7, alongside `@wterm/core` and `@wterm/ghostty` 0.5.7

## Lives in

`scripts/verification/tui/terminal/terminal-original.ts`, in the browser-side
original-PTY replay before the two animation frames and screenshot.

## Problem and evidence

With `autoResize:false`, the pinned package's `dist/wterm.js:409–410` calls
`_lockHeight()` during initialization. Its `resize()` at lines 659–684 changes the
core grid and schedules the renderer, but does not update that locked element
height. `_lockHeight()` at lines 1288–1298 computes rows times the measured row
height plus padding. Those locations are in the installed 0.5.7 dependency,
`node_modules/@wterm/dom/dist/wterm.js`, relative to `scripts/verification/tui/`.

A real captured 120x36→48x12 replay had the correct 12-row text grid while its DOM
rectangle and PNG remained 612 pixels high (36×17), leaving 408 blank pixels below
the 204-pixel terminal. A 32x24 replay likewise retained 612 instead of 408 pixels.
The original 108 PNGs and metadata are retained in the capture evidence; this
finding concerns screenshot geometry, not the native PTY's dimensions or bytes.

## Carried fix

After replaying the original bytes and actual resize boundaries, validate the
computed `--term-row-height` as finite and positive. Set the terminal element's
height to the final row count times that metric and reset its scroll position.
After the existing two animation frames, require the measured rectangle height
to equal that product within 0.01 CSS pixels. Existing exact browser/headless text
grid comparisons remain required. No dependency code is patched.

## Recreate and removal check

Use `renderOriginal` with retained ANSI and resize history that starts 120x36 and
ends 48x12 or 32x24, with `autoResize:false` as the established renderer uses. Before
the correction, inspect `rendered.height` in the `.original.json` receipt: it
remains 612 while the final row count is 12 or 24. Preserve the original bytes and
compare the PNG with the final grid; do not rerun the product to test this fix.

When upgrading wterm, check that upstream resize updates the locked height.
Delete our height assignment and scroll reset temporarily, keep the metric and
height assertions, and rerender the same resized grids. Remove the workaround
only when the assertions and original browser/headless grid comparisons pass
without it and the PNG dimensions match the final rows. Mark this entry removed
at the same time. The first-render evidence and all failure receipts stay kept.
