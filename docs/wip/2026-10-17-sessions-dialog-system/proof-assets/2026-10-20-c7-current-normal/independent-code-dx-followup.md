# Independent bounded code/DX follow-up

Verbatim returned final assessment. Root accepted the test-contract P3 after reading the actual assertion. Product/test correction remains with Main.

**Bounded result: one P3 test-contract finding; no runtime code defect confirmed. No whole-PR READY.**

Binding held from opening to closing: handoff `5f245f3c…`, freeze `6715ceb3…`, and all 11 immutable snapshots matched. Actual binaries also remained unchanged:

| Binary | SHA256 prefix | Bytes |
|---|---|---:|
| Component | `27cbcfac4011` | 300,956,272 |
| Router | `7c58652cf5e7` | 248,391,328 |
| Sessions | `93e88c177f4d` | 76,241,616 |

Full bindings are in the [target handoff](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/tmp/tui-implementation/c7-normal-options-default-target-handoff.json). UIWT’s browse-test file differs: frozen `96c7f917…`, current `8f11295b…`. Its added supported-page test received source review only; default9/full560 do not cover it.

**P3 — completed-entry proof overconstrains the permitted layout.** [quota_ux_options_entry_test.rs:85](/Users/shravansunder/code/projects/codex-router.codex-sessions-ux-redesign/crates/codex-router-cli/src/presentation/quota/quota_ux_options_entry_test.rs:85) requires `voucher-1` before scrolling. The supplied contract explicitly permits another meaningful initial fact and excludes a voucher-first layout requirement. A valid layout showing live eligibility initially, with vouchers reachable through scrolling, would fail this assertion. Main should assert the permitted semantic fact alternatives while retaining the chrome-only rejection and separate voucher-reachability checks.

The remaining deltas strengthen useful boundaries: real mounted input/layout, exact Alpha facts, full-frame return, held initial inspection and actual Normal publication. Fixture changes reverse exactly to the previously reviewed bytes after removing helper visibility. I reused unchanged coverage for eight files.

DX is an explicit prebuilt-consumer contract, with source/artifact checks, exact discovery and blank-image rejection. No ignored producer script is invoked by the inspected entry points. However, the journey reference and Python consumers are absent from HEAD: fresh-checkout reproduction has a **publication gap**. Fresh-machine execution remains unverified.

Retained, hash-matched logs support default **9/0** and feature-suite **560/0/7 across 11 targets**, plus lint/format/size closure. I executed none of them.

Moving Swift controls, original-image quality, browser/device key delivery, providers, CI and different-lineage coverage were not assessed. No writes, helpers or runtime actions occurred.


Publication note: private memory-citation markup was removed from this presentation copy; the review text and original product images are unchanged.
