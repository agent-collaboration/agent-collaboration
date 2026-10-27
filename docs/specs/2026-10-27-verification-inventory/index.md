# What happened to the verification code?

**The custom native-catalogue verifier is removed. The remaining TypeScript helper opens a real OS terminal; it does not yet test the requested menus.**

This inventory covers 103 verification-related paths accumulated during this task. At source revision `424fbc238`, 68 paths are absent, 23 contain surviving branch changes, 7 are unchanged from #173, and 5 moved with the crate split or were renumbered. Four moved files are identical to #173 at their new homes; the fifth is the branch-added node-pty note, renumbered UW-8. These are path counts, not test counts or 103 deleted programs.

- [What remains, and what it proves](current.html).
- [Every path, grouped by responsibility](files.html). Each group starts closed; open only the part you want.
- [Provider-menu design](../2026-10-26-acp-provider-menus/index.html). That is a separate document and an unfinished feature.

## Why the old system grew

The implementation began checking provider-owned files and plugin catalogues. Its verifier then needed matching file rules, installation qualification, fixture builders, process inspection, command reloads, client automation and independent result checks. That was a verification system for the rejected integration, rather than the small ACP bridge you wanted.

Some checks had a valid purpose: a test must catch a wrong result, a live run must identify its target, and cleanup must touch only its child. The mistake was keeping a large custom framework around the wrong product boundary. Its catalogue-specific readers, test fixtures, command vocabulary and duplicated oracles no longer earn their cost.

## Why was it under examples?

It was an executable Cargo example named `verify-provider-native-capabilities`, run with `cargo run --locked -p codex-router-host --example verify-provider-native-capabilities -- …`. It was a real verification program, not a harmless documentation sample. Its embedded test modules could be run for that example target. Sharing the executable's types and development dependencies explains the placement, but that rationale is inferred from the code; no recorded decision establishing that placement was found.

That entrypoint, its supporting modules, wrapper and native fixture skills are now absent. The project verification skill remains, with the old catalogue journey removed.

## The smaller verification boundary

`pty-process.ts` controls one real child terminal. Product tests exercise Router's own ACP handling. A future menu journey must connect the real client to the isolated Router and provider, select an action, and check the provider's result. Actual terminal-window captures establish visible menu membership. Neither unit tests nor raw PTY output can replace that last observation.

**Current result:** on Sunclaw at product source `424fbc238`, the TypeScript verification suite passed 848 tests, including 13 real OS PTY cases. ACP runtime checking and four submitted-input tests also passed. **Still unverified:** full Rust workspace gates, the new skill/slash menus and real-provider/screen behavior. One existing Rust unused-import warning remains.

Inventory basis: the 94-path native-verification snapshot at `5f5c49a4`, eight additional unique PTY-support paths and one later fixture repair. [Machine-readable path inventory](inventory.json). This is the verification work from this task, not every test or verification file in the repository.
