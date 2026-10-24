# Project description proof

Status: scoped implementation and required local gates passed; ready for the Lead to assess. The invocation head is 23ccc5e6 plus the scoped working delta. No post-commit immutable run or delivery acceptance is claimed.

## Contract

Description belongs to ProjectState, not the fleet directory. Creation accepts optional exact Description text and normalizes omission to empty, including the creation retry hash. Project describe uses normal project admission, an expected revision and keyed replay, with no project role. Project update changes only the fleet title. Project show returns the directory entry and Local state or NotLocal without creating an absent project file or introducing a remote read.

The unreleased project migration has one required TEXT column, with no DEFAULT, CHECK, ALTER or compatibility decode. Existing validated Description enforces zero through 16,384 UTF-8 bytes and preserves C0 controls. Native checked-query metadata and the real migration fingerprint were regenerated through their existing generators, both exit 0.

## Native execution

The first compilation stopped on an invalid fixture access to a private field, exit 101; no behavior ran. The fixture now retains the same FleetClient Arc it already supplies to admission and drops that handle before explicit shutdown. It does not open a second fleet owner or change the product API.

The corrected project_description target passed 4 / 0 / 0, exit 0 for that target. It covers exact maximum control-character creation, no-role update, stale revision with current revision, keyed retry with no feed advance, title-only fleet update preserving state, API/native reopen, empty/default canonical replay and conflicting reuse, directory-only NotLocal/no file creation, strict incoming bounds and strict current/feed decoding. The combined Cargo invocation exited 101 because the separate size fixture used revision 1 when INITIAL is 0. That refusal is retained as a fixture error, not behavioral red evidence.

After correcting the revision input, project_description_encoded_sizes passed 1 / 0 / 0, exit 0. All 3,861 before/after source hashes matched; runtime 108.31 seconds. It created 260 distinct maximum-description versions, encoded and validated the actual native captured pieces, encoded and validated every populated native snapshot page, then staged, verified and published the snapshot into a different native writer and read all 260 historical descriptions exactly.

| Measurement | Observed |
| --- | ---: |
| Description UTF-8 bytes | 16,384 |
| Captured pieces | 260 |
| Largest piece JSON bytes | 153,114 |
| Snapshot pages | 34 |
| History rows | 523 |
| Largest snapshot page JSON bytes | 10,904,281 |
| Largest history page JSON bytes | 10,904,281 |

The measured population uses an eight-byte version prefix followed by repeated NUL, SOH, newline and tab bytes. These are the largest observed encoded payload JSON sizes for that population, not a formal upper bound, full WebSocket frames or a network-limit claim. Snapshot page measurement includes schema statements where present. Count-based paging is unchanged. Copy acknowledgement and bootstrap grants are explicit fixture inputs; native capture, codecs, staging, verification, publication and durable history readback are real. Network deployment limits remain for G3/G4.

## Update-drop sensitivity and quality

The native update-drop mutant compiled and failed the intended exact readback assertion, 0 / 1, exit 101. It accepted the update and advanced the revision while keeping the original description. No assertion changed. Exact correct source bytes were restored, then the full description target passed 4 / 0 / 0, exit 0, with every source hash unchanged. This is post-implementation sensitivity, not pre-fix red.

All-target Clippy with warnings denied passed for message-board, message-board-storage, board-fleet-sync, collaboration-service, collaboration-client, collaboration-mcp, agent-collaboration and codex-router-host, exit 0. Its initial failures led to boxing the new Local payload (448-byte enum, same wire/schema) and replacing an explicit test panic with a propagated test error; the exact stored-value assertions remain. The restored native run qualifies those changes. MCP golden generation passed and changed only its intended snapshot.

## CLI, MCP and native suites

The actual CLI scenario passed 1 / 0 / 0, exit 0: create with description, show Local state, exact escaped-control request-file replacement, revision advance and keyed retry. The full MCP library suite passed 109 / 0 with two explicit installed-client/authenticated-provider ignores, exit 0. Its complete schema/handler parity matrix exercised 1,305 pairs: 29 accepted and 1,276 refused. The resolved catalog reported 171 tools, 343,384 input-schema bytes, 3,377,402 output-schema bytes and 110,314 project-input-schema bytes. These measurements do not assert provider-context acceptance.

The complete message-board, message-board-storage and board-fleet-sync suites passed 173 / 0 with one intentional child-process entry ignore, exit 0, across 39 nonempty targets. The earlier 14 board-sync-protocol passes belong to its separate C19 slice and are not added to this run. Cargo format and Git diff checks passed. All final executed commands retained 3,861 source hashes unchanged.

## Final gates and commit boundary

The complete native SQL metadata check passed, exit 0, without changing any source. The queued machine-wide `cargo test --workspace --no-run --locked --offline` passed, exit 0, in 641.63 seconds and compiled 594 test executables. No crate failed and all 3,861 before/after source hashes matched. This compiles the whole test surface; it does not execute the whole workspace suite. Only the previously accepted Apple compact-unwind size/performance warning appeared. The shared workspace slot was returned immediately; free space ended at 90 GiB, above the 45 GiB guard.

All Cargo commands used two build jobs, incremental disabled, dev/test debug information disabled, native SQLx offline metadata and the declared Apple SDK/Clang environment. The source invocation anchor remains 23ccc5e6 plus this delta. The final commit is qualified by comparing its complete non-proof source manifest to this observed state; no fresh post-commit immutable execution is claimed.

Successful behavior commands:

```sh
cargo test -p collaboration-service --test project_description --locked --offline -- --nocapture
cargo test -p collaboration-service --test project_description_encoded_sizes --locked --offline -- --nocapture
cargo test -p agent-collaboration --test project_description_cli --locked --offline -- --nocapture
cargo test -p collaboration-mcp --lib --locked --offline -- --nocapture
cargo test -p message-board -p message-board-storage -p board-fleet-sync --locked --offline
```

Quality commands:

```sh
cargo clippy -p message-board -p message-board-storage -p board-fleet-sync -p collaboration-service -p collaboration-client -p collaboration-mcp -p agent-collaboration -p codex-router-host --all-targets --locked --offline -- -D warnings
python3 scripts/tooling/prepare-sqlx-turso.py --check
cargo fmt --all -- --check
git diff --check
```

The measured 10.9 MB page prompted the Lead's separate library C18/W-C21 byte-budget work. Board record-kind maximums (512 KiB), trail-field limits and the reconstructed schema budget (256 KiB) are a separate follow-up, not silently included in this commit. The Lead has retained decisions over the other unbounded caller fields and derived history lists; no derived list is capped or truncated here. A5, A6, online two-Host and G2+ remain open. The Lead owns acceptance and delivery gates.
