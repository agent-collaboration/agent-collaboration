# S7 quota implementation checkpoint

The quota consumer uses the shared TUI controls, typed grouping and an optional UI preference store. Effectful proof uses opt-in fixtures with fake owned state; it does not use production accounts or the shared debug store. This checkpoint is source and library proof, not native, release or delivery acceptance.

## Canonical plan record

```text
plan path: ~/code/projects/codex-router.design-tui-component-system/tmp/plan-workflows/2026-10-30-tui-component-system-ui-pr-r2.md
originating planner: plan-implementation
planning result: ready
governing planning basis:
  kind: reviewed-four-document-design
  Requirements:        docs/specs/2026-10-29-tui-component-system/requirements.md
  Specification:       docs/specs/2026-10-29-tui-component-system/specification.md
  Program Design:      docs/specs/2026-10-29-tui-component-system/program-design.md
  Verification Design: docs/specs/2026-10-29-tui-component-system/verification-design.md
  four-document review: tmp/reviews/2026-10-30-tui-component-system-four-document-review.md
    (independent Sol xhigh reviewer; needs-revision at 9cbda49bf, F1–F6;
     same reviewer verified corrections at ff2629877: ready)
  current applicability anchors: design branch design/tui-component-system at fe8c97c53 (owner changes after review: crate renames, TS/Rust-only verification; review F7 original-image route);
    product base origin/main 881f778c7; iocraft pin agent-collaboration/iocraft 829ebd86
delivery context:
  requested terminal: pr-ready-unmerged
  delivery grouping: single:ui-component-system
  PR topology: one-pr
```

## Shipping behavior

R19 owns QuotaGroupFacet None/Provider/Status. F6 cycles the committed choice; Filters edits a draft with Apply/Cancel, and menu choices expose checked state. Rows filter before grouping. Provider order is OpenAI then Claude; status order is Preferred, Available, Held, Blocked, Stale, credential unavailable. A group heading shows count and typed active-client total; any unknown value makes that total unknown. Existing row order is preserved within each group. None preserves ungrouped order and paint.

Shared ListRows owns record order for paint, navigation, captured position and removal neighbors. Headings are not targets. Quota keeps ContentScroll for both row shapes; arrow navigation skips headings and reveals the selected record through the shared control. The quota consumer adds no focus repair or paging policy.

The CLI implements the typed preference port with the shared ui-preference-store crate. Opaque quota/group-by text lives in the optional UI database, separately from account state. Unknown/unreadable values load as None; a save failure keeps the run's choice. The actual CLI retains one serial writer across login remounts. Explicit Finished(latestfacet) closes new choices atomically, drains the last accepted value once and finishes despite retained snapshots. Dropping producers without the terminal signal is an error. Late choices are ignored.

T2 keeps the existing leading Details/Filters/Pool/Options/Floor toolbar block and adds menu-only View, Account, Refresh and Screen sections. Existing menu-order assertions are preserved. Phone and compact Help use actual InputOwner receipts and frame markers with causal bounds; keys, assertions, sizes and existing iteration limits remain intact.

## Five test-only seams

| Seam | Observation or capability | Owner |
| --- | --- | --- |
| 1. Refresh return/mapper | Actual service result, per-account outcomes or setup failure, equal consumed result and mapped UI result | CLI quota_refresh_adapter and observation helper |
| 2. Component/controller | Actual mount, Browse/list selection, captured dialog context, reload and post-draw publication | Quota TUI fixture_controller/fixture_view |
| 3. Exact-owned-root factory | Existing account command owner, typed completion and private clipboard across the CLI/login loop; rejects any other root | CLI quota_account_factory_test_support |
| 4. Parent wait | Actual child PID and raw wait code/signal/error before mapping, equal mapper input/result and parent-exit receipt | CLI quota_login_child and observation helper |
| 5. Preference save, observation only | Scope captured at actual adapter open; facet/ordinal and actual shared-store result after await, carried into the retained writer | CLI quota_group_preferences and observation helper |

These seams introduce no normal fixture callback or production environment switch. Parent/save/factory fixture paths require test, debug assertions and the existing harness feature; debug component support is gated from shipping builds. Release graph, symbol and raw-product exclusion is a separate required gate.

## Fixture ABI and additional routes

The original14 effectful scenario routes and11 read-only component profiles remain. The effectful entry is quota::quota_account_adapter::tests::effectful_fixture::compiled_quota_effectful_fixture, enabled only through the existing opt-in harness. QUOTA_EFFECT_FIXTURE_CONFIG names a strict JSON file. It requires an owned real PTY and private output paths. Setup ACKs are post-draw facts, separate from native dispatch and flushed-frame markers.

The fourth seam emits login-parent-exited with actual childPid, waitOrdinal and tagged exited/signaled/wait-failed status; final loginParentExit carries the same facts. Actual exit0/130/7 and raw status must agree with the mapper. A planned stand-in exit code alone is insufficient.

The fifth seam emits preference-save-returned with write:{ordinal,facet,result}; result is tagged saved/failed. Final preferenceWrites copies these actual facts and storedGrouping reads the actual database after CLI drain/close. Frozen readbacks expose actions[].checked without invoking disposed callbacks.

An additional repair-proof route uses the existing login-succeeded scenario and one native F6, retained readback, actual child0, fresh checked-Provider mount, then Quit. It requires exactly one actual Provider/Saved result. It adds no scenario key, config field or environment variable.

The additive15th scenario grouping-reopen runs the actual CLI twice sequentially in the same owned state and PTY: None→F6 Provider→Quit/drain/close; real store reopen Provider→F6 Status→Quit. Events grouping-invocation-started and grouping-invocation-settled carry actual mount/facet/store facts. Final groupingReopen requires mounts M then M+1 and stored provider then status; preferenceWrites requires exactly Provider/Saved then Status/Saved. Existing login guard fields are reused with an unbound private gate and exit0; grouping permits no child or unrelated effect.

The first grouping Quit must use its actual acknowledgement, real invocation settlement/store close, next invocation start/fresh mount and a marked frame for that mount. It is not process exit or a child handoff. Ending views do not need a fictitious flush marker.

## Evidence and open gates

Final four-library nextest run8411c392-e904-4e40-8835-ccfddd476783 passed747/747 with6skipped, exit0,179.079s, using two test threads. Packages: codex-router-cli, agent-router-quota-tui, codex-router-quota-service and ui-preference-store, with quota-reset-test-harness. Strict all-target Clippy across those packages plus codex-router-quota-reset-test-harness passed with warnings denied. The repository file-size gate passed3930files/maximum1000lines; final formatting and diff gates are recorded in the local proof manifest.

Preserved working-tree-only wrong controls fail for producer/consumer ordering, fabricated parent status, missing/wrong dispatch markers, provider order, missing preference publication, channel-close-only termination, duplicate save, post-terminal choice and dropped final save. Good hashes were restored before the passing gates; no seeded wrong build is committed.

Still required: current shipping-common release exclusion, qualified signed executables and exact manifests, actual CLI login-remount and grouping-reopen native profiles, protected Host diagnostic and workspace architecture gates. Historical or focused passes do not close these. No production Router replacement, shared configuration mutation, PR merge or release is authorized by this checkpoint.
