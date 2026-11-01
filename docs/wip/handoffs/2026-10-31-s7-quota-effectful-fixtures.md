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

Historical ea408 four-library nextest run8411c392-e904-4e40-8835-ccfddd476783 passed747/747 with6skipped, exit0,179.079s, using two test threads. Packages: codex-router-cli, agent-router-quota-tui, codex-router-quota-service and ui-preference-store, with quota-reset-test-harness. Strict all-target Clippy across those packages plus codex-router-quota-reset-test-harness passed with warnings denied. The repository file-size gate passed3930files/maximum1000lines; final formatting and diff gates are recorded in the local proof manifest.

Preserved working-tree-only wrong controls fail for producer/consumer ordering, fabricated parent status, missing/wrong dispatch markers, provider order, missing preference publication, channel-close-only termination, duplicate save, post-terminal choice and dropped final save. Good hashes were restored before the passing gates; no seeded wrong build is committed.

Still required: current shipping-common release exclusion, qualified signed executables and exact manifests, actual CLI login-remount and grouping-reopen native profiles, protected Host diagnostic. Historical or focused passes do not close these. No production Router replacement, shared configuration mutation, PR merge or release is authorized by this checkpoint.

## Retained migration diagnosis before 6ae

At the earlier stage, shared1cd9c52f3481c6ffd952ff0837123b3e2ac37b89 was merged uncommitted with the five legacy quota caller migrations, three terminal-hook Clippy prohibitions and actual R15 contracts. The matrix declares five real kinds at four grids (20contracts) and explicit TextField/FilterField/DialogKeyHints absences. Additional four Combo query journeys and a committing-switch availability journey cover all grids; no sixth seam. Existing literal assertions/goldens remain intact.

That stage's normal719run713pass6fail5skip/exit100 exposed five new Combo snapshot key-order oracles under preserve_order, since corrected without changing literal values; all four query cases pass. The old phone title journey waited for a repaint after Home at an already-visible top. It now uses the existing causal receipt/flush driver with all keys, five observations and the byte-identical original assertions/three-second guard; focused title passes0.564s. The matrix reaches its final Button32x24 case and fails the full word unavailable in the real sibling Yes reason. The shared owner is assigned that row-width repair; no consumer workaround or prefix rebaseline. Those observations are historical; the replacement source and current receipts follow below. No checkpoint was made over that red.

## Owner extensions and current source proof

Shared6ae942a35f753ddfa8fe34787a6b81f5f003ec72 replaces the pending1cd9 merge. The actual quota R15 matrix passes20real contracts at4grids, with3explicit absent kinds, plus16query and4committing-switch grid cases. Source-reused controller/routed observers add no sixth seam. The old phone title uses actual InputOwner receipt/flush markers with every original key and the byte-identical assertion/three-second-guard tail.

Owner test-layout instructions01a196b7 and01a196c1 produce exactly two harness targets: integration1 and e2e56. Before/after nextest list preserves57cases and5ignored. All23moved Rust source files preserve scenario/assertion bytes apart from required paths/selectors; the manifest test moves verbatim.20CARGO_BIN_EXE references are unchanged. PTY helper copies remain separate as documented in tests/README.md. The new exact observer entries are quota_storyboard::compiled_grid_terminal_observation and quota_storyboard::compiled_public_sessions_grid_observation. Existing held e729/0bfc/b191 artifacts remain unchanged; these are prospective caller names until a fresh signed declaration.

Owner design71efc57bb/2821bcb11/873f62923 and correctiona12f9c2db change only the interactive quota list: wider bar plus intact percentage, then Weekly · countdown/stale; Known5h is the fallback with a5h label, and absent/unknown windows show explicit muted Weeklyunknown without fabricated headroom. Printed Static/QStatic columns and JSON are unchanged; reported5h remains in Details. Five permanent new tests pass; a working-tree-only wrong selecting5h despite known weekly fails and exact good source is restored. No seeded wrong source is committed.

The normal-graph quota suite passes264/264,4skipped,exit0,34.352s with golden rewriting explicitly disabled. Each of the four owner-authorized browse golden deltas changes only two quota rows; all other rows and wide Account/Pace/Extra/Details portions are byte-identical. Identity, provider, client, credit, pace, capturedDetails, input, iteration-bound and timeout obligations remain. Current full library and quality receipts follow. Release exclusion and qualified native profiles remain open.

## Validated lane checkpoint, 2026-11-01

The full normal four-library run `5c8d144b-0444-49e6-b33b-f84cbb16775c` passes **724/724**, with 5 skipped (exit 0, 221.101s). Fixture-mode run `b1c53ccb-fe9d-4d35-a047-ebc8b526b0c2` passes **758/758**, with 6 skipped (exit 0, 260.997s). The later e2e-root annotation and README change no source compiled by these four libraries, including the borrowed loopback helper; exact source comparison is retained.

Strict workspace Clippy passes at exit 0 (341.583s); fixture all-target Clippy across the five affected packages passes at exit 0 (111.347s), both with `-D warnings`. The manifest integration test `9a565888-9d1c-40fa-b698-d9af2444fd89` passes 1/1, with 0 skipped (exit 0, 0.118s). Formatting, the 3,949-file size gate and cached diff checks pass. Driver tests pass 87/87 across 18 files, with type, lint and format checks at exit 0.

Final compiled inventory preserves all 57 exact case names and the same 5 ignored cases: integration 1 and e2e 56. All 23 moved Rust sources preserve non-path scenario and assertion bytes. Lead01a196c1 explicitly requires retaining the duplicated helper test instances; the new e2e root expects only the resulting `clippy::duplicate_mod` lint, with that reason. Every other warning remains denied. The initial duplicate-module failure is retained separately.

The first current architecture run `b8ef0368-ab06-4ffb-bea5-86a41330dfe0` timed out at the unchanged 300-second guard without a verdict. Lead01a19721 authorized one quiet run after S6b released Cargo. On the same 206 source hashes, `70e203a0-05d2-4b0a-a70d-0cc98e7a328d` passes **1/1**, with 22 skipped (exit 0, 236.798s): 50 packages, 50 targets, default 4 workers. Its baseline and guard are unchanged and successful per-package output is retained. The earlier timeout remains evidence; timing causation is not claimed. The quiet window has been released.

This source checkpoint is ready for the authorized lane commit/push. Next are the four raw shipping-common release controls, signed artifact qualification, actual CLI login-remount and F6 grouping-reopen profiles, and the protected Host diagnostic. No current native body, signed product declaration, PR merge, release or delivery acceptance is claimed.
