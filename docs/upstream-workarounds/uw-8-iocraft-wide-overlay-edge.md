# UW-8: a wide character consumes an overlay's serialized left border

**Status:** in place, 2026-10-30
**Dependency:** iocraft 0.9.2, fork revision `829ebd86bd07fe2100c096ec6d785875ec2e0c79`

## Lives in

`crates/agent-collaboration-tui/src/overlay_host.rs`, `SurfacePaintHook`.
The owner-approved component specification independently requires the gutter
on every covered row (2026-10-29 TUI component system, R10).

## Problem and evidence

When a two-cell character starts immediately left of an overlay, painting the
overlay overwrites the character's second cell but leaves its first cell. The
pinned `Canvas::write_row_impl` advances by the character's width and skips the
overwritten second cell, including a newly painted border or corner there.
Inspecting canvas cells alone misses the lost terminal border.

The permanent component test
`serialized_overlay_preserves_corners_border_and_background_with_a_wide_left_neighbor_on_every_row`
places each of 📂, ✉️ and 界 at column 19 on all six rows of an overlay beginning
at column 20. It reads the actual `Canvas::write_ansi` bytes through the layer-A
frame reader. The no-gutter build fails with an empty corner at `(20, 4)` while
the serialized row still contains the folder glyph beginning at column 19.
The full package run establishing that control had 49 passes, three intended
failures (two routing controls and this serialization control), and one skipped
measurement. This is component/serializer evidence; terminal backend diff and
resize writes remain the independent compiled-journey obligation.

## Carried fix

Before children draw, clear text and paint the surface background over its entire
rectangle and one extra column to its left. Use the current drawer position and
size, with the inherited clip. At terminal column zero, paint only the rectangle.
The root publishes hit regions after every child finishes drawing; the gutter
adds no target or input region.

## Recreate and removal check

From the workspace root, run:

```sh
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui
```

The correct build must preserve every left-border cell, both corners, the gutter
background and the closing redraw for all three glyphs. Removing the gutter must
fail the named serialized-frame test. Recheck this control after an iocraft
upgrade. A renderer that clears the leading half of an overlapping wide character
may resolve the underlying defect; R10's explicit gutter obligation still needs
an owner-approved specification change before its implementation can be removed.
