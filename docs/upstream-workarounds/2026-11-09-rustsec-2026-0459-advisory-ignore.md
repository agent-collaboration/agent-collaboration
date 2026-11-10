# RUSTSEC-2026-0459 advisory ignore on crates.io SQLx

[RUSTSEC-2026-0459](https://rustsec.org/advisories/RUSTSEC-2026-0459.html)
affects bundled SQLite before 3.54.0. Opening a database next to a crafted
sibling WAL file with a mismatched page size can expose uninitialized
page-cache memory. The workspace resolves libsqlite3-sys on the 0.35 line,
through sqlx-sqlite 0.9.0 and rusqlite 0.37.0, so its bundled SQLite is
affected.

## Real exposure

- **What the bug can do:** opening a database next to a crafted WAL file with a
  mismatched page size can expose uninitialized page-cache memory of the process
  that opens it. That is local information disclosure.
- **Where we open such files:** the Router's own databases in its state folder,
  and any file an operator passes to `agent-router-update import --source`. The
  import opens the named file wherever it lives, not only under the Router root.
  Opening it read-only and checking its schema afterwards do not protect against
  the underlying SQLite read bug.
- **How to stay safe until the fix:** only import legacy databases from a trusted
  source.
- **What this is:** the owner accepted the risk on 2026-11-09. It is not fixed.

## Why we stay on crates.io SQLx

Owner decision, 2026-11-09: stay on the crates.io release of SQLx (`=0.9.0`)
rather than pin an upstream git revision. No crates.io SQLx release permits
patched libsqlite3-sys 0.39 yet: published sqlx-sqlite 0.9.0 caps it below
0.38.0, and [SQLx #4502](https://github.com/launchbadge/sqlx/pull/4502), which
widens the bound, is unreleased. rusqlite stays at 0.37.0 so that one
libsqlite3-sys resolves across the graph. The advisory is ignored in
`deny.toml` (`[advisories] ignore`, with this reason) and in
`.cargo/audit.toml`.

## How to recreate

Remove `RUSTSEC-2026-0459` from both ignore lists, then run
`cargo deny check advisories` and `cargo audit`: both fail on the advisory.
`cargo tree --locked -i libsqlite3-sys -e normal` shows the single affected
version.

## When to remove

When a crates.io SQLx release permits libsqlite3-sys 0.39 or newer, upgrade
SQLx and rusqlite together, so that one patched libsqlite3-sys resolves. Then
drop both ignores, and require `cargo deny check`, `cargo audit`, and the
storage migration and reopen suites to pass.
