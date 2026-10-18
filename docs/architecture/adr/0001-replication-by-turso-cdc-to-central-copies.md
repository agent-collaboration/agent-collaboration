# ADR 0001: Replicate each project by Turso change capture to its own central copy

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-08). **Design:** requirements U32–U33; spec 2 WA-3, WA-3a, WA-3b, WA-12; program design 2 §4.

## Context
Each project is one Turso database with one writer, its holder machine. The first design replicated it through Turso Sync's push and pull to a central hub. Testing showed two problems. A push isn't all-or-nothing: a failed step is skipped while its cursor advances, so a replica can diverge silently. And Turso's sync server is documented for development and testing only.

## Decision
- The holder captures its own committed changes with Turso's change capture (CDC).
- It groups them into whole transactions and sends them, in order, as pieces over one session per machine.
- Each project has its own central copy, a Cloudflare Durable Object. The copy applies a piece all-or-nothing, only if it continues from the copy's acknowledged position, and refuses pieces from a superseded epoch.
- Schema changes travel as the migration's own SQL.
- A new holder is seeded from the central copy.
- Other machines read a position-labelled snapshot of the copy.

## Consequences
- We gain atomic, ordered and idempotent replication that we control end to end, with no Sync server and no cross-project ordering (no Lamport clocks).
- We pay for it by owning the piece protocol, the decoding of Turso's change rows, and the retention of the change table until each change is acknowledged.
- Turso's change capture is young (0.8.x). The version is pinned and re-checked on upgrade.

## Evidence
- Turso 0.8.1 change capture, tested on 2026-10-08: committed transactions are grouped and marked by commit markers; rolled-back work leaves no rows; a decoded replay matched data and schema.
- The Sync push atomicity tests (2026-10-07).

## Revisit when
Turso ships an atomic push and a production sync server, or change capture misses a write path we use.
