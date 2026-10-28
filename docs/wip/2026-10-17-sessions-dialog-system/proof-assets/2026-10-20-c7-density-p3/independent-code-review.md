# Bounded C7 density P3 independent code/proof review

One confirmed **P2 proof defect**. No additional confirmed production defect in the declared density correction. This is an affected source/test/quality review, not whole-PR acceptance. The reviewer has no product authorship; Root owns disposition and acceptance.

## P2 · DENSITY-P3-F1: modal crop can silently admit the outer frame

Anchor: [frozen quota_ux_clarity_test.rs:128](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/tmp/tui-pr-draft/c7-density-p3-frozen-review/after/crates/codex-router-cli/src/presentation/quota/quota_ux_clarity_test.rs:128), corresponding to the current source at the same line. The affected algorithm extends through line 148.

`actual_dialog_text` chooses the first complete bottom border below the title using only whether its horizontal bounds enclose the title. It then searches upward for a top-right corner matching that chosen bottom. This does not independently establish that either border belongs to the modal.

Concrete source-derived counterexample: retain a complete inner modal top/title inside the ordinary rounded outer Quota frame, but omit/clip the inner modal bottom. The outer bottom encloses the title, so line 132 accepts it. The inner top has a different right column and is skipped at line 137; the outer top matches. Lines 139–148 consequently return the outer frame interior, including background account facts. With a complete inner top, the previous implementation instead required the bottom at the inner left column and rejected this missing-bottom case.

This matters because the helper is the attribution boundary for actual Options task content and Help/captured identity. For example, [quota_ux_options_entry_test.rs:176](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/crates/codex-router-cli/src/presentation/quota/quota_ux_options_entry_test.rs:176) accepts `Use credits` plus `Disallow` from the returned crop, while [quota_ux_help_test.rs:92](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/crates/codex-router-cli/src/presentation/quota/quota_ux_help_test.rs:92) collects identity/count evidence from it. A malformed/clipped modal can therefore borrow facts from outside its bounds instead of failing the observation. That weakens the active-dialog attribution obligation in U5/U11 and the correction brief's prohibition on relying on an obscured background row. It does **not** establish that the current product renders a broken modal or that the retained positive runs failed.

Minimum correction belongs to this existing test helper: independently bind the modal geometry around its title, retain support for the overwritten top-left cell, and reject a missing/mismatched modal bottom rather than expanding to the outer frame. Add a boundary negative with a complete outer frame, absent inner bottom and background-only sentinel; retain a positive for the real wide-underlay case. These are requested corrections, not edits made by this review. The counterexample was inspected analytically; no test or mutation was executed.

## Production and test assessment

The full 565-line diff and declared after-files were inspected, including the owning formatter, JSON projection, browse renderer, test helpers, mounted paging tests and four goldens. Prior unchanged product evidence remains bounded to its earlier target.

- Reset counts retain their typed count/context; the renderer removes only the redundant reset glyph. Real quota bars, measured pace meters, state colors, impact labels and Exhausted behavior remain in their existing owners.
- [quota_status_value_formatting.rs:78](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/tmp/tui-pr-draft/c7-density-p3-frozen-review/after/crates/codex-router-cli/src/quota/quota_status_formatting/quota_status_value_formatting.rs:78) constructs semantic nonbreaking unit groups from typed windows. It adds no model, padding or rendered-string parser. Guarded unsigned subtraction preserves future/past reset durations. Missing reset, unknown, stale and ineligible meanings remain distinct through the shared status match. JSON's original `window_display_note` and detail formatting retain their original text behavior. Unavailable pace alone becomes semantic text without a decorative empty meter.
- Fixture edits change presentation spacing/copy, not quota percentages, reset times, balances or identities. All four golden numeric-token sequences match their before versions; measured meters and selected-detail facts remain. Short static output remains explicitly clipped rather than being presented as complete interactive proof.
- Paging tests still require weekly/5h values, compact credits, row-owned pace and exact return frames. The constrained initial frame retains identity/Weekly, while the actual 83% value must be reached during paging. The separate `alpha pace` sentinel remains. Reverse paging's ownership inference is limited to this unchanged two-row fixture. No-op adjustments first return to the witnessed End pose, then exercise Home/PageUp; they do not replace the equality checks or row-arrow roundtrip. DENSITY-P3-F1 limits claims that depend on the changed modal crop.

No new public API, dependency, lifecycle or numeric decision boundary appears in this diff. The focused command selectors remain usable with the declared package/features; a prebuilt default artifact is correctly distinguished from the feature-enabled test executable. This review does not close the existing publication/fresh-checkout reproduction gap.

## Retained execution evidence inspected; nothing rerun

The [final quality record](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/tmp/tui-implementation/c7-density-p3-final-quality.json) and its linked phase receipts/raw logs were inspected. Sixteen linked receipt/log hashes matched. Relevant results:

- Old production with the new unit tests: actual 1 passed/2 failed, exit 101, exposing separated count/unit facts; the unavailable-meter test separately failed 0/1, exit 101. The initially named `units-first-green` actually failed 3/1 and remains a failure. The subsequent unit run passed 4/0, exit 0; its formatter binding is intermediate, not silently relabelled final source.
- Final-source tests are covered by the retained completed library phase (550/0/5 ignored) plus the remaining targets. The composed scope is **565 passed, 0 failed, 7 ignored across 11 targets**, not a single successful whole-package command. The original parent was terminated at 90.07 seconds, exit -15, and remains failed.
- Fourteen inputs are byte-identical across the composed phases. The fifteenth changes only UTF-8 prefix indexing to checked `get(...).expect(...)` at `str::find` boundaries; the saved before-lint source and complete diff support the claimed equivalence. The final helper/paging source has a separate actual 4/0 current run, exit 0. This equivalence does not cure the crop algorithm finding.
- Strict all-target CLI Clippy with `keychain-test-support`/`-D warnings`, format check and size check each exited 0. The size log reports 2,049 Rust files within its 1,000-line cap; this is a mechanical gate, not an architecture verdict.

## Binding and limits

Opening/closing checks matched all 30 frozen before/after copies; the closing live 15 files also match every declared after hash. No full hash dump is needed: the packet is the exact manifest.

| Bound input | SHA-256, unchanged at closing |
| --- | --- |
| packet.json | `1616324c27799c6ad1fe42ee9dbeb1e2a0bab0fb1a5f3a907d554da443c03d1b` |
| affected.diff · 565 lines | `a05ed7e96c18a337917e42ab39f1c38715e8cbb35fe3624ad6e389f6fef0fa24` |
| final quality | `aff02d0291310fac58cc0cf1bc18434f117287b27e945f19009772f6448dc30a` |
| product artifact state | `2139f57bfcd0639ee79f57ae9bff671812cc641ea2534278004d3d634f9a58b7` |
| default component | `e873c3a6873eb4852e1d7bc12455989de9067fe26f354783f134ecb7dda23228` |
| Router | `cf68fa036368396bda04125fdb2b3afaeb1c555e7b509950d2e7622fe8579561` |
| agent-sessions | `93e88c177f4d5eecafeb641f8f1fb05ed2b8df1ba3c604037c4fc1ffcd14c354` |

All three artifact files were independently hashed twice and match. The actual Cargo JSON events identify the default component executable/profile and successful no-run build; all 15 after hashes also match the build's 2,284-input manifest. I did not independently rehash the entire dependency superset. The prior reference is the immutable 060307 product27cb plus separately bound test deltas, not clean HEAD. Decisive unchanged Options/Help callers matched the build manifest and remained unchanged during inspection.

Worst case from the confirmed finding is a false-positive modal-content/identity assertion masking missing or misplaced UI facts. No production corruption or wrong-account effect was demonstrated. Changed crop/source/test bytes require affected re-review and proof binding; changed binaries, build inputs or evidence logs invalidate corresponding evidence claims.

No builds, tests, UI, provider/network calls, Git operations, helper/wake actions or moving V apparatus inspection occurred. Only this requested report was written. Default-nine gates on e873cf and fresh originals at all six grids remain pending for this commission; old product27cb captures cannot establish its appearance. Visual quality, device/provider behavior, CI, fresh-checkout delivery and different-lineage final review remain open. This report neither waives those gates nor accepts the whole PR.
