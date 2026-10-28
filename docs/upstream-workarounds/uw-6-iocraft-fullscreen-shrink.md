# UW-6: iocraft clears the visible bottom row after a canvas shrinks

**Status:** in place, 2026-10-25
**Dependency:** iocraft 0.9.2, pinned fork revision `829ebd86bd07fe2100c096ec6d785875ec2e0c79`
**Upstream:** not yet reported. ccbrown/iocraft 0.9.3 and `main` still lack the guard;
no issue or pull request has been filed.

## Lives in

The workspace `Cargo.toml` pins `agent-collaboration/iocraft` to the
[backend correction and its regression tests](https://github.com/agent-collaboration/iocraft/commit/829ebd86bd07fe2100c096ec6d785875ec2e0c79).
The carried fix is in the dependency's `packages/iocraft/src/backend/crossterm.rs`.

## Problem and evidence

The fullscreen diff writer compares the previous and next canvases up to their
maximum height. When the previous canvas is taller than the current terminal,
clearing its old tail emits cursor positions below the visible screen. A terminal
that clamps those positions to its last row can clear the bottom border just
painted there. This path is reachable when the backend already remembers the new
terminal size, so its size-change full rewrite does not run.

The dependency's permanent `fullscreen_shrink_regression_retains_bottom_after_stale_tall_canvas`
test exercises the real backend's ANSI output through its existing virtual
terminal. It requires the title, footer and complete bottom border after the
shrink and after another unchanged frame. Three controls cover a size-change
rewrite, an equal-height diff and explicit full-rewrite recovery. This reduction
establishes the backend defect; it does not establish the cause of every retained
historical screenshot failure.

## Carried fix

Stop the fullscreen diff loop when the absolute output row reaches the current
terminal height. Visible rows keep their normal diff behavior. The workspace
consumes the reviewed fork revision rather than modifying Cargo's cached source.

## Recreate and removal check

In a checkout of the pinned iocraft revision, run its permanent backend tests:

```sh
cargo test -p iocraft fullscreen_shrink_ -- --nocapture
```

Require all four tests to pass. Removing the row-bound guard must fail the stale
tall-canvas regression while the other controls retain their stated behavior.
When an upstream release contains the correction, repeat those checks against
that release, replace the fork pin with the released dependency, and rerun this
repository's complete compact Quota/Sessions capture matrix at all six supported
terminal sizes. Remove the pin only after the native image checks and original
screen review pass; mark this entry removed at the same time.
