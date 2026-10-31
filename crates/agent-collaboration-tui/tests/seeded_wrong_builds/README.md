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
| `typography-ignores-covered-context.patch` | Shared headings, body text and column rules retain uncovered colors beneath an overlay. |
| `list-rows-focusable.patch` | Tab walks individual list rows before reaching the toolbar. |
| `list-selection-kept-by-index.patch` | Reload changes the selected identity when the same key moved. |
| `list-navigation-in-consumer.patch` | A consumer input chain changes the covered list beneath an overlay. |
| `form-minimum-label-width-ignored.patch` | Separate sections place their values at different columns despite the shared minimum. |
| `raw-unit-separator-not-normalized.patch` | The raw Help key misses Ctrl+7 and can produce an unprintable chord label. |
| `content-scroll-pages-selection.patch` | ContentScroll page/wheel input changes the selected key instead of only scrolling content. |
| `list-marker-offset-ignored.patch` | A folded group's marker is drawn on its heading rather than the configured row. |
| `receipt-unexpected-action.patch` | An unrelated input unexpectedly invokes the real Inspect action; the independent effect count changes. |
| `receipt-resize-as-click.patch` | A delivered Resize is recorded as a mouse press. |
| `receipt-drop-ack.patch` | A routed event produces no complete receipt. |
| `receipt-duplicate-ack.patch` | An event produces duplicate receipt lines/sequences. |
| `receipt-surface-after-close.patch` | The new top surface hides the overlay or parent that actually consumed the event. |
| `receipt-before-callback.patch` | The receipt becomes visible before on_routed returns. |
| `receipt-text-leak.patch` | Printable text is serialized instead of its payload-free category. |
| `content-dialog-forced-maximum.patch` | A fitting form fills its maximum height instead of sizing to current content. |
| `content-dialog-retains-cross-body-height.patch` | Help's outer height persists after the original body is restored. |
| `menu-uses-toolbar-label.patch` | Menu renders the toolbar-only short label in place of the action's full title. |
| `toolbar-budget-uses-full-title.patch` | A short button drops off although its painted label and key fit. |
| `toolbar-label-cached-at-install.patch` | A live toolbar label stops updating after the action is installed. |
| `quit-alias-dropped-in-help.patch` | A declared screen quit alias is ignored by the real Help surface. |
| `quit-alias-hardcodes-control-d.patch` | Ctrl+D globally quits an undeclared scope and steals its own domain binding. |
| `raw-control-c-not-normalized.patch` | Raw U+0003 misses the shared Ctrl+C chord and cannot quit from a dialog. |
| `combo-filter-editor-full-width.patch` | The filter editor measures the whole row rather than its remaining width and clips the query tail/cursor. |
| `menu-no-widen.patch` | A requested narrow menu clips its title instead of expanding into available viewport space. |
| `menu-auxiliary-starves-title.patch` | Unbounded key/reason columns force an action title to wrap at a narrow viewport. |
| `menu-all-aliases.patch` | Menu paints the full alias list instead of the primary key; Help is the complete alias projection. |
| `hover-reveals-partial-row.patch` | Hover selection scrolls to reveal a partially visible tall row. |
| `hover-activates-row.patch` | Pointer motion invokes the selected row's action. |
| `hover-enabled-by-default.patch` | A list that never opted in changes selection and focus on pointer motion. |
| `modified-arrows-change-focus.patch` | Unbound modified arrows fall through to grouped focus traversal. |
| `pinned-context-gap-omitted.patch` | Pinned dialog context touches the scrolling body without its one blank row. |
| `empty-context-has-gap.patch` | An empty context leaves a redundant blank row and excess content height. |
| `pages-only-consumes-arrows.patch` | Form arrows scroll the body instead of moving field focus. |
| `pages-only-ignores-pages.patch` | Home and page keys cannot scroll back from a field revealed by focus. |
| `button-focus-no-bold.patch` | A focused button loses its shipped bold label. |
| `button-focus-no-underline.patch` | Focus loses its non-colour underline signal. |
| `button-checked-style-ignored.patch` | A checked button loses yellow/bold emphasis when focus moves elsewhere. |
| `button-unshipped-selection-background.patch` | Focus or checked state adds a selection background absent from the shipped buttons. |
| `help-heading-accent-normal.patch` | Help section headings use Accent/Normal instead of the shipped Primary/Bold style. |
| `dialog-heading-accent-normal.patch` | Dialog section headings lose their shipped Primary colour and bold hierarchy. |
| `field-label-accent.patch` | Form labels use Accent instead of their shipped Primary colour at normal weight. |
| `header-wrap-before-body.patch` | Wrapping header consumes the first body fact at 48x12. |
| `header-keeps-help-hint.patch` | Help hint stays while context needs the space. |
| `header-context-no-ellipsis.patch` | Clipped context loses its ellipsis. |
| `header-title-no-ellipsis.patch` | Overlong title loses its ellipsis. |
| `header-context-gap-starves-body.patch` | Pinned gap consumes the final body row at 48x10. |
| `content-dialog-retains-help-padding.patch` | A restored form retains automatic padding selected from the previous Help frame. |
| `list-typeahead-inserts-actions.patch` | A printable action key is inserted into the shared filter instead of reaching its declared binding. |
| `list-typeahead-moves-focus.patch` | Editing through a list moves focus to the field. |
| `list-typeahead-drops-text.patch` | Typeahead consumes text or paste without editing the shared state. |
| `list-typeahead-reserves-escape.patch` | Cancel receives Esc before a nonempty shared filter can clear. |
| `pinned-context-truncated-with-room.patch` | Context is truncated beside the title even though its own row fits with three body rows. |
| `pinned-context-body-minimum-two.patch` | A second pinned row steals the third body row. |

| `pinned-context-measures-allocation.patch` | A short identity is measured as an allocated 60-cell child and moves to a second row despite fitting inline. |
| `pinned-context-splits-tag.patch` | Name and tag truncate together, clipping a tag that fits the row. |
| `pinned-context-omits-header-rows.patch` | Content sizing omits the second pinned header row and squeezes a fitting body. |
| `content-dialog-ignores-measured-extent.patch` | A content-sized frame uses intrinsic auto-height instead of its measured current body and chrome. |
| `content-dialog-retains-header-width.patch` | Content-sized chrome retains a narrow draw width and never restores fitting Help after resize. |

| `combo-secondary-omitted.patch` | The dropdown omits a choice's secondary text. |
| `combo-secondary-forced-inline.patch` | Overflow secondary text stays beside its label instead of using an indented muted line. |
| `combo-secondary-wrong-colour.patch` | Secondary text loses the shipped muted foreground role. |

| `right-edge-focus-marker.patch` | Fitting lists, menus, and dropdowns add a duplicate focus glyph in the right scrollbar column. |

| `menu-keeps-requested-height.patch` | Menus keep a tall requested height after their final item instead of fitting their content. |

| `content-height-undercount-one.patch` | A content-sized frame subtracts one measured body row and scrolls despite spare room. |

Receipt controls require `--features test-support` with a debug test profile.
The existing outside-fallthrough control also challenges the real receipt and
the independent screen-effect count after an outside menu press. The three
input-owner controls were mechanically refreshed for RouteDecision without
changing their defects: frozen availability, frozen surface snapshot and
outside-click fallthrough.

These controls are implementation evidence; the independent S5 observation is
separate. Check each patch applies, then challenge it individually against the
full package at its declared source target. Runtime assertion failures must
identify the named defect. Compile errors, application errors, color setup
failures, and entry/redraw timeouts do not establish sensitivity.

The earlier `content-dialog-forced-percent.patch` and
`content-dialog-no-measurement.patch` seeds were retired during the initial
compact-header correction, when native Auto matched those declared fixtures.
The later continuing Checking-to-Result scenario exposed a different intrinsic
height after a body swap. Native Auto now bootstraps an unmeasured body; current
body and chrome measurements supply its bounded target height. The existing
ignore-measured-extent seed challenges that case; undercount-by-one challenges
the final wrapped line. Forced maximum and cross-body height controls remain,
with every original form, section, overflow, action, and spacing assertion.

Pinned context now hard-cuts over to typed text and an optional tag. The shared
crate owns natural measurement and rendering; caller elements and allocated
widths are no longer accepted. The arbitrary-child clipping and stale child
measurement seeds were retired with that contract, replaced by the allocation,
whole-tag, measured-header and resize controls above. Narrow titles yield to a
whole tag; long names use one ellipsized pinned row. The approved Options wide
golden now uses its natural width with Help fitting, retaining its 24-row height,
body capacity and actions. Existing wide and tiny Details goldens remain exact.

T4 replaces the old same-row secondary clipping rule with a full secondary line when it does not fit. The Lead approved retiring `combo-secondary-not-truncated.patch` in favor of `combo-secondary-forced-inline.patch`; omission, muted style, secondary-only filtering, real pick/close and full-label checks remain.

| Patch | Intended failure |
| --- | --- |
| `combo-option-label-wraps.patch` | An overlong dropdown label wraps instead of remaining one ellipsized row. |
| `combo-option-reason-same-row.patch` | An unavailable reason crowds its label instead of occupying a muted indented line. |
| `combo-option-reveal-composite.patch` | Focus reveal omits the expanded option block extent and clips the reason. |
| `menu-reason-not-right-aligned.patch` | The menu reason leaves space before its primary key instead of aligning to that column. |
| `menu-reason-crowds-title.patch` | A natural-width menu leaves only one separating cell between title and reason. |
| `menu-reason-wrong-colour.patch` | The menu reason loses the muted foreground role. |
| `field-reason-inline.patch` | The disabled reason shares the editor row and steals its width. |
| `field-reason-wrong-colour.patch` | The disabled reason loses its muted foreground. |
| `field-reason-nowrap.patch` | A disabled reason clips instead of wrapping below the editor. |
| `button-always-shows-key.patch` | A button prints its primary key despite its local hidden-hint preference. |
| `button-hidden-key-drops-binding.patch` | The hidden-hint scenario loses the live Left binding while metadata and Help retain it. |
| `dialog-body-bottom-aligned.patch` | A fixed-height dialog puts spare blank rows above its body instead of below it. |

T9 changes the disabled editor fixture from one row to two so its reason can occupy the approved next row; the value style and ignored-input assertions remain. T7 replaces the former fixed-dialog body/footer adjacency assertion with a pinned-footer and empty-space-below-body assertion. Content-sized forms retain their exact one-row footer gap, and fixed frame sizes remain asserted.

| Patch | Intended failure |
| --- | --- |
| `switch-paints-partial-group.patch` | Paint full switch labels and keys despite the shared remaining budget. |
| `switch-hidden-target-remains-focusable.patch` | The group is hidden but its targets remain eligible for Tab. |
| `filter-minimum-on-inner-child.patch` | The consumer reserves its minimum only inside the child and budgets the full peer row for the switch. |
| `form-value-refuses-reflow.patch` | The shared label column squeezes a value instead of moving above it. |
| `form-selector-gutter-one-too-wide.patch` | An extra selector cell breaks a 36-cell UUID that fits after the correct two-cell gutter. |
| `status-detail-starves-word.patch` | The detail text squeezes the status word into fragments. |
| `help-columns-refuse-reflow.patch` | Fixed Help columns fragment the captured account, action titles and conventions. |
| `help-native-word-breaks-token.patch` | Native Unicode line-break opportunities split fitting paths and bracketed tags. |
| `help-overlong-token-never-wraps.patch` | A token longer than the whole body clips instead of hard wrapping. |
| `help-token-width-stale-on-resize.patch` | A continuing Help body uses its old token-width budget after resize. |
| `help-collapses-original-spacing.patch` | Help token layout collapses original multiple spaces. |
| `help-keyless-placeholder.patch` | A keyless Help action gains an invented dash key. |

The inner-child minimum control mutates the sample consumer allocation, including its shared remaining-width budget; all expected assertions stay unchanged. The toolbar full-title budget control follows its existing calculation into the shared Button width owner. Header/field style controls are mechanically refreshed for the native reflow layout.

| Patch | Intended failure |
| --- | --- |
| `focus-no-automatic-fallback.patch` | A disabled or dropped target stays current before any Tab. |
| `focus-never-returns-displaced.patch` | An eligible displaced target never regains focus. |
| `focus-returns-after-user-move.patch` | Automatic return overrides a newer explicit focus choice. |
| `focus-pointer-keeps-old-return-intent.patch` | A pointer focus move leaves the displaced return intent active. |
| `focus-model-refresh-drops-return-intent.patch` | Replacing the complete dialog model preserves only its current ID and loses displaced intent, including a Help-suspended model. |

The switch resize fixture now asserts focus repair immediately before Tab. With automatic repair, a later Tab on the sole eligible filter makes no visual change; its focus assertion remains, and its incorrect redraw wait is removed. Existing render-time snapshot and availability-cache controls are mechanically refreshed around active-focus reconciliation.

The existing complete-model focus-reset and discarded-primary controls are also mechanically refreshed around intent transfer; they retain the same faults and original assertions.
