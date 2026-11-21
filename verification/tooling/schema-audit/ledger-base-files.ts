/**
 * The source-coverage ledger's base file list, committed as data so the ledger test needs no git
 * history: the base commit is not in the public repository after the D0 squash, and CI checks
 * out shallow (correction 6).
 *
 * Generated once from commit afc76c82cfad3375e2861b1720084aae0530a6f4 (`git ls-tree -r --name-only afc76c82c`) by the rule
 * `LEDGER_BASE` records: paths under every crate's and provider package's `src` directory (`crates/<name>/src/`, `providers/<name>/src/`), leaving out by
 * name only colocated unit tests (`*_test.rs`, `*_tests.rs`), sorted. Split into parts to
 * keep every file under 1,000 lines. The provider settings entries follow their relocation
 * and split in PR 291. The Host custody machine and three relocated machine paths are listed
 * in their owning crate.
 */
import { files as part1 } from "./ledger-base-files/part-1.ts";
import { files as part2 } from "./ledger-base-files/part-2.ts";
import { files as part3 } from "./ledger-base-files/part-3.ts";

export const LEDGER_BASE_FILES: readonly string[] = [...part1, ...part2, ...part3];
