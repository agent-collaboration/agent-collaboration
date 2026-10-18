# ADR 0005: Central applies each piece inside the official workers-rs transaction API

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-11: use supported APIs, proven by experiment). **Design:** program design 3, C8.

## Context
Applying a piece must be all-or-nothing: check the fence, write the rows, move the acknowledged position. Cloudflare forbids `BEGIN` and `SAVEPOINT` through `sql.exec`, and a multi-statement `exec` isn't atomic. workers-rs doesn't wrap `transactionSync`.

## Decision
Central uses the official workers-rs `Storage::transaction()`. It doesn't reach `transactionSync` through `as_raw()` or reflection. The core's work runs inside it as a synchronous closure, with no `.await`.

## Consequences
- We use only supported APIs.
- The transaction blocks every other event to the object until it finishes, so a fence can't land mid-apply.
- The core still can't pause inside a transaction.

## Evidence
A local test on 2026-10-11 (workers-rs 0.8.7, workerd 2026-10-06, a real compiled Worker):
- it commits on success;
- it rolls back on a SQL error, a Rust `Err` and a fence mismatch, including across an `.await`;
- another request to the same object ran only after the transaction finished.

The matching workerd source wraps the transaction in `blockConcurrencyWhile`. Cloudflare's documentation says SQL inside `transaction()` is part of the transaction on SQLite-backed objects. A permanent integration test with no mocks, plus a deployed staging check, are required before production.

## Revisit when
The deployed check disagrees, or workers-rs adds `transactionSync`.
