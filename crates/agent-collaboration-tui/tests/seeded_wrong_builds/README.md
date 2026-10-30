# Component sensitivity controls

Each patch breaks one contract. They are test inputs, never product features or
runtime switches. Apply one from the workspace root, run the **entire package**,
then reverse that same patch:

```sh
git apply crates/agent-collaboration-tui/tests/seeded_wrong_builds/duplicate-key.patch
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui
git apply -R crates/agent-collaboration-tui/tests/seeded_wrong_builds/duplicate-key.patch
```

| Patch | Required failure |
| --- | --- |
| `duplicate-key.patch` | Duplicate scope keys accepted. |
| `restore-index.patch` | Reordering restores the old position instead of the captured identity, in both the pure model and routed closing scenarios. |
| `wide-menu-glyph.patch` | Menu glyph is two cells in the current width table. |
| `permissive-frame-reader.patch` | Unknown CSI and OSC 8 accepted; CSI K leaves stale row content. |
| `separated-burst.patch` | The burst driver's acknowledgements observe redraws between input events. |
| `render-time-gate.patch` | Burst input reaches beneath a newly opened overlay; focus then Enter invokes the old target. |
| `previous-frame-rect.patch` | The published map has no target on frame 1, although the real canvas has drawn cells. |
| `crate-only-clip.patch` | Cells outside a plain consumer overflow clip are registered. |
| `no-gutter.patch` | A wide left neighbor consumes the serialized overlay corner. |
| `outside-fallthrough.patch` | Closing a menu invokes the target beneath it. |
| `undimmed.patch` | Covered foreground/background roles retain their normal colors. |
| `stale-scroll-map.patch` | A click in the same burst after scrolling uses an invalid map. |
| `no-ctrl-c-opt-in.patch` | The backend intercepts Ctrl+C before InputOwner acknowledges it. |

All thirteen controls were replayed separately through the full S3 package suite.
Each compiled and exited 100 with its named runtime failure. Compile errors,
patch-application errors, and color setup failures do not count as sensitivity.

Check each patch applies before replay. Later slices must refresh a patch when
its owning model changes and establish its named full-package failure at the S5
target. A failed application is an evidence gap, never a passing control.
The independent S5 observation remains separate from these implementation runs.
