# DENSITY-P3-F1 affected independent recheck

**VERIFIED CLOSED — DENSITY-P3-F1/P2.** No additional confirmed P0–P3 finding in the offered helper repair and proof. This closes the previously reported modal-crop defect only; Root retains acceptance ownership.

The complete frozen before file, after file and diff were read. The before bytes exactly match the helper reviewed in the original finding. The change is confined to that test helper and four permanent boundary tests; the other fourteen presentation inputs remain unchanged.

## Why the correction closes the finding

[after.rs:163](/private/tmp/2026-10-tui-component-system/modal-crop-final-review-20261020T082539Z/after.rs:163) selects the nearest enclosing top border before searching below the title. Its right corner is measured in Unicode display cells; the contiguous one-cell horizontal border determines the left column independently of the bottom. This preserves the declared CJK case where a wide underlay glyph covers the top-left corner.

[after.rs:179](/private/tmp/2026-10-tui-component-system/modal-crop-final-review-20261020T082539Z/after.rs:179) then requires both bottom corners at those already established columns. With the original counterexample—complete inner top/title, absent inner bottom, complete outer frame—the outer bottom cannot match the inner columns. The helper rejects rather than expanding to background facts. The displaced-bottom case likewise rejects; it cannot redefine the crop's left edge. The extraction still excludes both border columns and uses Unicode cell positions. No bottom-derived fallback remains on this affected path.

The four new tests exercise outcomes rather than internal calls: two invalid crops must reject, while proper and CJK-overwritten-corner crops must retain the title/Close and exclude background `Use credits`/`Disallow`. The positive cases prevent an always-panicking helper from satisfying the negative tests.

## Retained execution evidence inspected, not rerun

| Evidence | Actual result and binding |
| --- | --- |
| Causal RED, `actual_dialog_crop` | 2 passed / 2 failed, exit 101. Both valid cases passed; missing and mismatched bottoms incorrectly returned instead of rejecting. |
| Corrected focused GREEN | 4 passed / 0 failed, exit 0. Both negative cases panic at the matching-bottom boundary and are caught as expected. This run binds the pre-format helper `d30bfb89…`, not the final bytes. |
| Final `quota_ux_clarity_test` run | 37 passed / 0 failed / 0 ignored, exit 0; 522 filtered out. Both source maps bind final `9672875d…`. Includes all four crop tests and real Help, Options, identity, paging, resize, form and confirmation callers. Retained 76×30 Help/Options output was inspected. |
| Final strict CLI Clippy | `--locked --offline -p codex-router-cli --features keychain-test-support --all-targets -- -D warnings`, exit 0, 8.42 seconds. |
| Final format / size | `cargo fmt --all -- --check`, exit 0; size check exit 0, 2,060 Rust files within its 1,000-line cap. |

The RED source hash `78b2299c…` was independently reproduced in memory by combining the exact before helper with the four new tests; it matches the RED receipt. Thus the causal failure is bound to the old algorithm, not an unrelated broken fixture. The final 37-test run independently covers the post-format source, so closure does not depend on treating the earlier GREEN as byte-identical.

The original 25.05-second Clippy attempt remains timeout/exit -15. Its immediate receipt retains two exiting clippy-driver entries; the separate cleanup receipt records their later absence without additional signals. The successor and final gates record empty remaining groups. These are inspected retained observations, not a new live process inspection by this reviewer.

## Binding and limits

Opening and closing hashes matched the handoff and all eighteen referenced source/diff/receipt/log files. The current owned source also matches the after hash at both checks; the other fourteen live inputs match their declared unchanged hashes. Final affected/quality source-before and source-after maps agree on all fifteen inputs.

| Frozen input | SHA-256 |
| --- | --- |
| handoff.json | `3276c293b6b4fc82ebf5c5e8c3d639bd37492f308bced26cc329bc1b9651062d` |
| before.rs | `2408271e4362d42495ce439bbabe1733460a49ce2f414b9e858c69474b0d90f8` |
| after.rs | `9672875d4dc1dea84425bad220a77a14b4aecb3754905be5a50757073078bd6b` |
| affected.diff | `b8a13e43e790bcafbe9dd855ad675aba42e1410a23f42edb10c70c2966ce4445` |

The retained 2,284-input compiled manifest contains the old `2408271e…` helper and the same other fourteen inputs. The archived component/Router/Sessions files were independently hashed and still match `e873c3a6…` / `cf68fa03…` / `93e88c17…`. Those binaries, the earlier completed 550-test library phase and earlier default-nine evidence are **not** relabelled as containing the new helper or four tests. The current 37-test execution supplies the affected repair proof. The packet's broader 2,283-unchanged-input claim was not independently rehashed in full during this narrow recheck.

Unchanged coverage from the preceding density audit is preserved; this is not a new production, apparatus or whole-PR review. The four boundary cases do not claim a general parser for arbitrary corrupted frame/title text. Current screenshots and UX results supplied as context were not accepted as appearance evidence here. Visual/device/provider behavior, fresh-checkout delivery, CI, different-lineage final coverage and whole-PR acceptance remain separate.

Changes to the bound helper, its tests or retained proof require affected rebinding/review. No tests, builds, UI, provider/network calls, Git operations, helpers or wakes were run. Only this requested report was written.
