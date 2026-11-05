# Test organization: Verification Design

Each requirement is proven on the real build: Cargo's dependency views, nextest's case inventories, and CI-machine builds measured before and after under one fixed decision rule. Nothing here has been observed yet.

## Proven by (owner view)

| Need | Proven by | Stays unverified |
| --- | --- | --- |
| U1 · parts with owners | Only the two named Cloudflare codec paths still reach into another crate's source. The shipping build's dependency tree contains no test-support crate and no `test-support` feature. | those two paths, until the production design |
| U2 · CPU and cache | The test build is measured twice before and twice after on the same CI machine type, in four scenarios: cold, cached-dependency, test-only edit and production edit. A fixed rule decides "worse" and "better" for each measure, so the same numbers always give the same verdict. | other machine types; developer laptops |
| U3 · rules for agents | The rules are in AGENTS.md. Each enforced check fails on a deliberately planted violation. | whether agents read the rules |
| U4 · proof kept | Test inventories from the tree before the move and the tree after it match exactly, for every CI test selection the move touches, the separate e2e runs included. | — |
| U5 · validated first | The Advisor's check before authoring, and this independent design review. The owner then decides the production split separately. | — |

## Coverage rows

| U | R | Observation | Target | Passes when | Fails for |
| --- | --- | --- | --- | --- | --- |
| U1, U3 | R1 | O1 source-path inventory | the PR head | exactly the two R1 exceptions remain | a third cross-crate edge; an exception whose file or target is gone |
| U1, U3 | R2 | O2 shipping closure | natively on Apple Silicon (release job, and the main-push warm-up job at `ci.yml:512`): `cargo tree --locked -p codex-router-cli -p agent-collaboration -p board-legacy-upgrade -e normal,build,features`, the release selection (`release.yml:147`) | no `*-test-support` crate and no `test-support` feature in the output; a dev-only activation, such as collaboration-mcp's `collaboration-service/test-support`, is absent from it | the same activation moved into a normal dependency of any of the three roots |
| U1, U3 | R3 | O3 manifest facts | `cargo metadata` for every package | `autotests = false`; integration and e2e targets ≤ 1 each; README present | a second integration target |
| U4 | R4 | O4 inventory pair | pinned pre-extraction tree P and post-extraction tree Q, same toolchain, six selections: workspace (`ci.yml:310`, build `:295`), quota-reset (`:304`, build `:299`), isolated port cutover (`:316`), and the three e2e runs (`:389-391`) | for each selection, the run and skipped identity sets observed on Q equal those observed on P | a case dropped or renamed, for example the loopback case or a `scenario_finalization` case; a dropped e2e case |
| U2 | R5 | O5 measurement pairs | the CI machine type in use (Blacksmith 16 vCPU if adopted), two samples per side per scenario | the decision rule below | a clear CPU, link or cache regression; a "gain" inside the noise |
| U3 | R6 | O6 seeded red | working tree only, never committed | each enforced check fails on one planted violation, and passes after it's removed | a check that can't fail |

## Observations

- **O1:** the include inventory, resolving each `#[path]` against its declaring file. It's the R1 check's output if the owner accepts Q1, and otherwise a review-time command.
- **O2:** the `cargo tree` output above, from the native Apple Silicon release job. A Static `--target aarch64-apple-darwin` view is an early warning only: its build-script and proc-macro side is the Linux host. A Darwin-conditioned production activation must fail the native check. Cargo metadata isn't used for R2, because its resolver merges dev units.
- **O3:** `cargo metadata --format-version 1 --no-deps`.
- **O4:** `cargo nextest list --message-format json` per selection, once on P and once on Q, and the two identity sets compared. P's inventory is recorded before any edit.
- **O5:** each scenario sets its starting state explicitly, as follows.
  - **Cold:** rust-cache disabled for the job, on a fresh runner.
  - **Cached-dependency:** the normal restore of main's entry.
  - **Test-only edit:** after the cached build, in the same job, a one-line change to one test file, then rebuild.
  - **Production edit:** the same with a one-line change to a collaboration-service source file.

  `CARGO_INCREMENTAL=0` as in CI. Every sample builds exactly the nextest selection the CI test step uses (`ci.yml:305`). The `cargo test --timings` step's selection isn't used, because it differs.

  Measures:
  - **CPU:** user+sys, from `/usr/bin/time -v` around the build.
  - **Elapsed:** wall time.
  - **Target size:** bytes of `target/debug`, from `du -sb`.
  - **Saved-cache size:** the compressed rust-cache entry, from the cache API.
- **O6:** one planted violation per enforced check, recorded in the PR description by its output.

## Decision rule for R5

For each measure m and each scenario:
- **The samples:** take two before samples, b1 and b2, and two after samples, a1 and a2. Let B = mean(b1, b2) and A = mean(a1, a2).
- **The noise band:** N = max(|b1 − b2|, |a1 − a2|, 3% of B).
  - For saved-cache bytes, which hardly vary between runs, N = 1% of B.
- **Worse** means A > B + N. Any "worse" fails R5 for the change being measured.
- **Better** means A < B − N.
- **R5 passes** only when no measure is worse in any scenario, and at least one measure is better in at least one scenario.
- **Inside the band:** a difference within ±N counts as neither worse nor better.

Example: with b1 = 100, b2 = 104 and a1 = a2 = 101, B = 102 and N = 4, so A = 101 is neither worse nor better.

## Limits

R5 is a gate, not a forecast. If phase 1 is better on no scenario, the owner gets the numbers, and the hygiene case is judged on its own merits.
