# Architecture decision records

One record per decision: the context, the decision, what it costs, the evidence it rests on, and what would make us revisit it. A record is never edited after it's accepted. A later decision supersedes it with a new record that names the old one.

| ADR | Decision | Status | Date |
|---|---|---|---|
| [0001](0001-replication-by-turso-cdc-to-central-copies.md) | Replicate each project by Turso change capture to its own central copy, not by Turso Sync | accepted | 2026-10-11 |
| [0002](0002-project-leases-with-fencing.md) | A lease registry with fencing: 4-hour leases renewed every 15 minutes, writes refused after expiry, holder changes only through an admin endpoint | accepted | 2026-10-11 |
| [0003](0003-central-core-with-platform-shells.md) | Central's rules live in a platform-free core; storage is the only injected interface; Cloudflare and a local server are thin shells | accepted | 2026-10-11 |
| [0004](0004-rows-keyed-by-primary-key.md) | Replicated rows are keyed by their declared primary key, with full capture on every connection and writes only through admission | accepted | 2026-10-11 |
| [0005](0005-official-durable-object-transactions.md) | Central applies each piece inside the official workers-rs `Storage::transaction()` | accepted | 2026-10-11 |
| [0006](0006-turso-lock-waits-in-our-driver.md) | Turso is used as published. Our driver works around its lock-wait spin; blocking disk IO and plain `fsync` are accepted | accepted | 2026-10-11 |
