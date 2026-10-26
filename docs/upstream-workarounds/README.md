# Upstream workarounds

This is the log of fixes we carry in our own code for defects in libraries we depend on. Each entry holds the evidence, how to recreate the problem, what our code does, and the check that says the fix can be removed once upstream fixes the defect.

Add an entry when we write such a fix. Re-run its check when that library is upgraded. When the fix is deleted, mark the entry `removed` and keep it.

| ID | Library | Defect | Our fix | Status | Re-check when |
|---|---|---|---|---|---|
| [UW-1](uw-1-turso-busy-wait-spin.md) | Turso Rust binding 0.8.x | While waiting on a database lock, `step()` wakes itself immediately and spins, burning more than a core for the whole wait | Our `sqlx-turso` driver sets `busy_timeout = 0` and retries "busy" with a Tokio timer backoff | in place | upstream issue #7211 closes, or we upgrade Turso |
| [UW-2](uw-2-turso-upsert-cdc-before-image.md) | Turso 0.8.1–0.8.3 | A conflict update captures its before-image after rewriting the row | Project relations use explicit checked INSERT or UPDATE under the existing writer | in place | a released engine containing #9396 passes the native before/after-image checks |
| [UW-3](uw-3-turso-parser-stack-depth.md) | Turso parser 0.8.1 | Moderately nested SQL exhausts the default native debug thread stack before its depth guard | Checked metadata queries use one subject kind and omit the extra CTE wrapper | in place | a released bounded or iterative parser passes the default-stack recreation and service regressions |
| [UW-4](uw-4-turso-left-join.md) | Turso 0.8.1 | After reopen, an outer-only LEFT JOIN predicate permits a partial index to omit valid outer rows | Body batches join by primary key and select the kind condition with CASE | in place | the direct SDK reduction and reopened product/read-cost proofs pass with the original join |
| [UW-5](uw-5-worker-sql-storage-size.md) | worker0.8.7 | SQL database bytes narrow to 32-bit usize on wasm32 | Read the existing raw getter and validate an exact u64 count | in place | the SDK exposes lossless bytes and the conversion/Worker Health checks pass |
| [UW-6](uw-6-node-pty-macos-spawn-helper.md) | node-pty 1.1.0 | Published macOS spawn-helper lacks execute permission | Build the native dependency from source through its supported install script | in place | a fresh normal install passes the real-PTY suite without the override |
