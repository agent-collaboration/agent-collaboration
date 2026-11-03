# UW-11: negative half-cell origins inflate the scroll range

**Status:** in place, 2026-11-01
**Dependencies:** Taffy 0.5.2 through iocraft 0.9.2, fork revision
`829ebd86bd07fe2100c096ec6d785875ec2e0c79`

## Evidence

The native Help fixture at 120×36 has 34 painted content rows in a 14-row
viewport. End correctly moves to offset 20. After that translation, the same
body is reported as 35 rows, although its last text line is still at logical
row 33. The thumb stops one row early, and Down advances to offset 21 and
exposes an empty bottom row. The permanent tests use an independently declared
`HELP-END` line; their expected endpoint is not derived from the measured range.

Taffy's `round_layout` rounds the cumulative top and bottom separately before
subtracting them, using `f32::round`. At a negative half-cell origin, an integer
height spanning zero can gain a row: `round(-9.5 + 34) - round(-9.5) = 35`.
The same height at a positive half-cell origin remains 34. The pinned iocraft
drawer instead accumulates separately rounded local positions. The resulting
extra layout row has no corresponding painted content.

Decisive source paths are Taffy 0.5.2 `src/compute/mod.rs:194–200` and
`src/util/sys.rs:35–36`, and the pinned iocraft `src/render.rs:274–292`.
Native run `58117b32-9610-4440-935b-a29f01cdf3e0` records the unchanged body's
34→35 height change and the two runtime assertion failures. Moving measurement
to another wrapper did not solve it: descendants inherit the cumulative origin.
Removing Help's section gap also left the extra row; shipped spacing is retained.

## Carried fix

`surfaces/overlay_host.rs::centered_cell_padding` assigns an odd unused cell to
the leading padding when a centered axis has an already resolved cell length.
The remaining free space is even, so flex centering produces an integer origin.
This preserves the previously rounded screen position, including asymmetric
margins, without changing the public placement contract or the renderer pin.
Content-sized axes retain their existing centering policy.

## Recreate and removal check

From the workspace root:

```sh
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui \
  -E 'test(help_end_and_last_reach_the_final_painted_line_and_scrollbar_end_at_every_grid) | test(help_down_cannot_advance_beyond_the_independently_painted_final_line)'
```

Both tests drive the real render loop at 120×36, 76×30, 48×12 and 32×24, with
burst and frame-separated input. End and Option+Shift+Down must show the final
painted line and the thumb endpoint together; a subsequent Down must not move
past that line. The timeouts are failure guards, never settling delays.

On an upgraded Taffy/iocraft, remove only the parity adjustment in a working
tree and run both tests. Remove the workaround only when they pass without it,
the full crate checks pass, and centered surface positions remain unchanged.
Keep this entry and mark it removed. No dependency fork or pin change is part
of this workaround.
