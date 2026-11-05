# Test organization: Verification Design

Each requirement is proven on the real build: Cargo's metadata, nextest's listings, and CI-machine builds measured before and after. Nothing here has been observed yet.

## Proven by (owner view)

| Need | Proven by | Stays unverified |
| --- | --- | --- |
| U1 · parts with owners | The boundary check finds no source path into another crate, apart from two named exceptions. The layer check finds no production dependency on test support. | the two Cloudflare codec exceptions, until the production design |
| U2 · CPU and cache | The test build is measured before and after on the same CI machine type: cold, cached-dependency, test-only edit and production edit. CPU time, link-inclusive time, target size and saved-cache size must not get worse on any of them, and must improve on at least one. | other machine types; developer laptops |
| U3 · rules for agents | The rules are in AGENTS.md. Each check fails on a deliberately planted violation. | whether agents read the rules; the checks catch what they can |
| U4 · proof kept | Test lists from the same commit, before and after, match exactly for both CI test invocations. | — |
| U5 · validated first | The Advisor's check before any build, and an independent design review. The owner then decides the production split separately. | — |

## Coverage rows

| U | R | Observation | Target | Passes when | Fails for |
| --- | --- | --- | --- | --- | --- |
| U1, U3 | R1 | O1 boundary assert | workspace at the PR head; `cargo nextest run -p workspace-architecture-check` | zero `#[path]` targets outside their crate, except the exact two allow-listed files | any remaining include, or an allow-list entry that no longer matches a file |
| U1, U3 | R2 | O2 metadata | `cargo metadata` with the release features of codex-router-cli, codex-router-host and collaboration-service | no normal or build edge into a `*-test-support` crate, and no `test-support` feature enabled | a fixture crate in a production dependency tree |
| U1, U3 | R3 | O2 metadata | every workspace package | `autotests = false`; integration and e2e targets ≤ 1 each; README present | a second integration target |
| U4 | R4 | O3 list pairs | same revision, both CI invocations' exact selections | run and skipped identity sets equal | a case dropped, renamed or newly skipped |
| U2 | R5 | O4 E5 pairs | Blacksmith 16 vCPU (or the CI machine type chosen), same toolchain, features and inventory; `/usr/bin/time` on the cargo build, `--timings`, `du` of target, the size of the saved rust-cache entry | no scenario worse beyond run-to-run noise (measured by repeating the before build once), and at least one better | an extra-link regression, or a larger cache |
| U3 | R6 | O5 seeded red | working tree only, never committed | each check fails on one planted violation and passes after it's removed | a check that cannot fail |

## Observations

- **O1:** the R1 check's own output, listing every resolved target.
- **O2:** `cargo metadata --format-version 1` read by the R2 and R3 checks.
- **O3:** `cargo nextest list --profile ci --message-format json` pairs, reusing the CI-to-30 design's comparison method.
- **O4:** one CI run before and one after, plus one repeat of the before run to measure noise. Scenarios:
  - cold, with no cache;
  - dependency-cached, which is the normal CI case;
  - a one-line edit to a test file;
  - a one-line edit to a collaboration-service production file.
  The inventories match because both sides build the same selection. The experiment's earlier timing step built the e2e targets the nextest selection excludes; the measured step uses the nextest selection itself.
- **O5:** a planted violation per check, recorded in the PR description by its output, never committed.

## Limits

R5 is a gate, not a forecast. If phase 1 shows no gain on any scenario, the owner sees the numbers, and the hygiene case stands or falls on its own.
