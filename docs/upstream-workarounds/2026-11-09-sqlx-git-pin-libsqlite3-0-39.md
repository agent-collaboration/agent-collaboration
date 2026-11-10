# SQLx pin for patched bundled SQLite

**Status: removed.** Owner decision 2026-11-09: stay on crates.io SQLx 0.9.0
with rusqlite 0.37.0 and ignore RUSTSEC-2026-0459 instead; see
[UW-14](2026-11-09-rustsec-2026-0459-advisory-ignore.md). The record below
describes the pin as it was.

The workspace patches the seven locked upstream SQLx crates to
[`10462fa4414c6c7f1ed1d39bb31678e3b726878c`](https://github.com/launchbadge/sqlx/commit/10462fa4414c6c7f1ed1d39bb31678e3b726878c)
while retaining their `=0.9.0` dependency declarations. Together with rusqlite
0.41.1, this resolves one libsqlite3-sys 0.39.1 (SQLite 3.54.1) across the graph.
The external sqlx-turso and sqlx-durable-object drivers retain their own pins.

[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
affects bundled SQLite before 3.54.0: opening a database and a crafted sibling
WAL with mismatched page sizes can expose uninitialized page-cache memory.
libsqlite3-sys 0.39 contains patched SQLite. No published SQLx release currently
permits that version; published sqlx-sqlite 0.9.0 caps it below 0.38.0.
[SQLx #4502](https://github.com/launchbadge/sqlx/pull/4502) raises the upper bound
to `<0.40.0`, allowing the patched library without a local fork or an advisory
ignore.

The pinned revision also rejects unrecognized `.sql` filenames in migration
sources by default. The workspace accepts that upstream behavior unchanged:
active migration sources use numeric version prefixes, and the two legacy
account SQL scripts load through `include_str!`. The checksum calculation,
version parsing and ordering, applied/missing migration reconciliation, and
SQLite migration-history DDL match released 0.9.0.

To recreate the dependency conflict, remove the SQLx patches while retaining
rusqlite 0.41.1 and resolve the lockfile: the two drivers' libsqlite3-sys ranges
cannot unify. The former lockfile with rusqlite 0.37.0 resolves vulnerable
libsqlite3-sys and fails `cargo deny check advisories` and `cargo audit` on
RUSTSEC-2026-0459. Check the adopted graph with
`cargo tree --locked -i libsqlite3-sys -e normal`, then run the storage migration
and reopen suites and the external drivers' consumer checks.

Remove the patches when a crates.io SQLx release permits libsqlite3-sys 0.39 or
newer and compiles with the pinned external drivers. Update the exact SQLx
version declarations together, resolve one patched libsqlite3-sys, and require
the workspace checks, migration/reopen suites, and advisory checks to pass.
