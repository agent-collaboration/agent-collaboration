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

| `native-text-input.patch` | The native widget handles covered text and paste, or loses the routed cursor. |
| `cached-per-render-availability.patch` | A second press in one burst runs an action after the first makes it unavailable. Only action availability is cached; overlay and focus snapshots remain fresh. |
| `hand-typed-menu-label.patch` | The menu rectangle no longer contains the registry's title/key pair. Background labels cannot satisfy this assertion. |
| `combo-first-match-reset.patch` | Still-matching typing loses the current keyed highlight. |
| `combo-highlight-by-position.patch` | Reorder/removal changes the highlighted identity. |
| `double-section-gap.patch` | Sections have two blank rows between them. |
| `missing-section-gap.patch` | Sections have no blank row between them. |
| `missing-action-gap.patch` | Dialog actions have no blank row above them. |
| `combo-targets-built-once.patch` | An option added while open has no clickable target. |
| `combo-stale-highlight-click.patch` | A click after reorder/removal picks the prior highlight rather than the row under the pointer. |
| `combo-stale-parent-update.patch` | A retained control mutates the closed dialog's draft. |
| `pointer-rows-focusable.patch` | Tab leaves the filter for an option row; Enter must pick the keyed highlight instead. |
| `publish-invalidated-paint.patch` | Focus reveal invalidates the paint's map, yet that old map is published. |
| `help-does-not-restore-model.patch` | Returning from Help loses the saved dialog focus/input model. |
| `cached-help-availability.patch` | Help no longer reflects the source action's live unavailable reason. |
| `focused-button-enter-inert.patch` | Enter no longer activates the focused primary button. |

| `title-cached-at-install.patch` | An installed title remains frozen after consumer state changes. |
| `primary-update-discarded.patch` | An open model update fails to change the primary action. |
| `model-update-resets-focus.patch` | Rebuilding an open model loses its prior focused TargetId. |
| `closed-model-update-calls-updater.patch` | An update invokes consumer code after its overlay closed. |
| `help-conventions-dropped.patch` | Help loses the consumer-supplied input conventions. |
| `help-keeps-saved-base-model.patch` | A saved Help copy hides updates to the dialog's continuing model. |
| `model-update-writes-help.patch` | Updating the dialog replaces Help's own routing model. |

These controls are implementation evidence; the independent S5 observation is
separate. Check each patch applies, then challenge it individually against the
full package at its declared source target. Runtime assertion failures must
identify the named defect. Compile errors, application errors, color setup
failures, and entry/redraw timeouts do not establish sensitivity.
