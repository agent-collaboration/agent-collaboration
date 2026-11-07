# Test organization: Requirements

Tests now make up most of the Rust code in the largest crates: collaboration-service has 91k lines of tests against 38k of product code. Shared fixtures move between crates by `#[path]` source includes, and one test-support crate pulls the whole CLI, proxy and state stack into every consumer. The owner asked for test organization first, as a design validated before any production-crate split, followed by rules that let future agents place tests correctly without judgment calls.

## Who is affected

| Class | Job | Current pain | Evidence |
| --- | --- | --- | --- |
| Owner | authorizes structure changes; pays CI minutes | test code is hard to see as parts with owners; CI compiles the same fixture source several times | 38 cross-crate `#[path]` includes (listed in [specification.md](specification.md)) |
| Agents writing tests | decide where a test and its fixture go | no rule says when shared test code needs its own crate, so agents copy files across crates by path | `crates/agent-collaboration/tests/integration/main.rs:5`, `crates/codex-router-host/tests/integration/main.rs:3` |
| CI (and its CPU minutes) | builds every test binary on each run | shared fixtures recompile inside each consumer; `codex-router-test-support` drags CLI/proxy/state into consumers that need only process ownership | `crates/codex-router-test-support/Cargo.toml:12-20`; `board-legacy-upgrade` uses only `owned_cli_process` and `scenario_finalization` |

## Needs

| ID | Need | Why it matters | Authority | Priority |
| --- | --- | --- | --- | --- |
| U1 | Test code is organized into parts with a clear owner and contract ("chunks that make sense, not blobs") | readers and agents can find, own and change tests | `authorized`: owner 2026-11-05 | 1 |
| U2 | Build CPU minutes and cache use are optimized even where minutes are free, without abusing cache or runner limits | CI cost and speed | `authorized`: owner 2026-11-05 ("in CPU minutes even if it's free we need to optimize; make sure cache is used for builds and we don't abuse the cache or any limits") | 1 |
| U3 | Written rules that future agents can apply without judgment calls: where a test goes, when shared test code gets its own crate, where test support lives | prevents drift back into blobs and path copies | `authorized`: owner 2026-11-05 ("propose changes to how we organize tests as a system so future agents follow right principles") | 1 |
| U4 | No test loses proof: same cases, inputs, assertions, deadlines, scheduling groups and causal waits | the reorganization must not weaken CI | `authorized`: standing owner rules (no weakened proof; no wall-clock tests) | 1 |
| U5 | The design is checked by an Advisor first, and is validated before any production-crate split, which the owner authorizes separately | the owner wants the principles proven on tests first | `authorized`: owner 2026-11-05 ("use advisor to check any splitting of test crates first"; "make proposal and design that's validated for production crates (which I will authorize)") | 1 |

Constraints in force: the owner's test layout rule (unit tests in sibling `*_test.rs`; integration and e2e only under `tests/integration/*` and `tests/e2e/*`, one `[[test]]` target each with a README). Files stay under 1,000 lines. TypeScript or Rust only.

## Boundary

- **In scope:** test-support code and its crates, `#[path]` includes that cross a crate, test-target layout, the rules and the architecture checks that enforce them, and measuring build CPU, link time, target size and cache size before and after.
- **Protected:** test cases and their proof (U4); production crate structure, which is the owner's later decision (U5); the B2 files in collaboration-central-core, in flight for the Fleet access Lead.
- **Non-goals:** splitting production crates; moving test cases into new system-suite crates in this change (a later pilot, see Q2); CI runner choice, which is the Blacksmith decision.

## Open owner questions

- **Q1 · The path-include check.** Enforcing "no cross-crate `#[path]`" needs a check that reads `#[path]` attributes in source. The owner has called repo-wide text-scan tests pedantic and preferred targeted boundary asserts. This design scopes the check to one boundary assert: `#[path]` targets that resolve outside the declaring crate. Status: proposed; the owner decides whether it stays.
- **Q2 · A system-suite crate.** Journeys that span CLI, MCP, Host and central could live in one named crate (for example `collaboration-system-tests`). The Advisor warns it is not a proven CPU gain, and that moving tests loses same-package `CARGO_BIN_EXE_*` discovery. Status: deferred to a measured pilot after phase 1.
