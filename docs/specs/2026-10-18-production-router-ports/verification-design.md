# Proof of default selection

Reference: [Specification](specification.md), R1–R4. These are required observations, not completion claims.

| Row | Deciding observation | Wrong behavior that must fail |
| --- | --- | --- |
| R1 | Parse omitted Host options and test the production-used effective binding. First observe the old 8788 value fail against the specification's 19742 oracle, then run the same test after correction. Keep the existing production proxy 19741 assertion. | A helper-only test disconnected from `run_host_command`, a duplicated formula or an explicit 19742 argument cannot establish omitted-default selection. |
| R2 | Retain loopback rejection and table coverage for normal/debug omission, explicit port 0 and an explicit nondefault bind. | Debug drift, dropped overrides and non-loopback acceptance fail. |
| R3 | Retain existing Host replacement tests comparing the actual captured argument list. Do not restart production to test a default. | Restart silently choosing different captured ports fails. |
| R4 | Inspect the exact guide/version/lock diff and run format, the affected CLI suite, strict Clippy and repository size/policy/current-head CI gates. Independent verification and implementation review follow on the current target. | An unrelated dependency refresh, stale executable evidence or an old release version cannot pass as this delivery. |

Local commands include `cargo test --locked --offline -p codex-router-cli --lib`, `cargo fmt --all -- --check`, `cargo clippy --locked --offline -p codex-router-cli --all-targets --features keychain-test-support -- -D warnings`, and `python3 scripts/tooling/check-rust-file-size.py`. The test-support feature preserves the repository's guard against spawned acceptance binaries using the production Keychain adapter.

Use isolated build storage and bounded compiler jobs; sequence broad workspace gates and captures. The paired Verifier checks offered sensitivity, source/build identity and the absence of production effects. Existing reciprocal 0.1.96 Tailnet evidence proves the exposure path, not this changed compiled default. Required CI and review remain delivery gates. The implementation plan ends PR-ready and unmerged; publication and manual production activation remain separate steps.
