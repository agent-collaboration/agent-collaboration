# ADR 0004: Replicated rows are keyed by their declared primary key

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-11). **Design:** program design 2, C12 and §5; program design 3 §4.

## Context
Turso's change capture identifies rows by SQLite's hidden rowid. Our tables are keyed by UUIDv7 text primary keys, so they also carry an implicit rowid, and that rowid can be renumbered. Capture is also switched on per connection, not per database.

## Decision
- **Rows by key.** Central upserts and deletes rows by each table's declared primary key. Capture runs in `full` mode, so a delete carries its key.
- **Capture on every connection.** The project store opens every connection in one place, with capture switched on. Only admission and the migration runner can write.
- **Schema limits.** Every project table declares a primary key. Triggers and virtual tables are refused when a migration is built.

## Consequences
- Rowids never have to match between holder and central, so vacuum is harmless and seeding is simpler.
- Capture records more bytes on updates and deletes.
- A write that bypasses Router entirely (for example, `sqlite3` on the file) can't be refused, only detected. A drift check is deferred to a standalone script, by the owner's choice.

## Revisit when
The extra bytes from `full` capture matter, or a table needs a trigger.
