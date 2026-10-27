# Legacy fixture repair

This later repair isolates existing SQLite test inputs; it does not change production upgrade behavior or add a second verifier.


## upgrade_prepare.rs

- **Path:** [crates/board-legacy-upgrade/src/upgrade_prepare.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/board-legacy-upgrade/src/upgrade_prepare.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Copies shared SQLite inputs into per-test temporary directories before the existing upgrade-reader checks.

- **Why this change remains (source-based explanation):** Isolates shared fixture readers without weakening the production open-file guard.
