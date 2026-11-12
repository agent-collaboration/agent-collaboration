# sqlx-durable-object

The owned SQLx 0.9 driver for a Cloudflare Durable Object's SQLite storage. It moved
here from agent-fleet-sync (source b75d072, unchanged through c84a078); the root
`Cargo.toml` patches durable-steps' git dependency to this copy, so one driver links
into the Worker.

Use `DoConnection::new(state.storage())` inside an object. Both shared and mutable
connection references implement SQLx's executor. Transactions use
`DoConnection::transaction`, backed by `Storage::transaction`; SQLx's ordinary `begin`
is refused because the platform owns the transaction lifetime.

`migrate::run_migrations` runs SQLx's stock `Migrator` inside that transaction (the
idea of agent-fleet-sync c217f0b, without durable-steps), so a migration's SQL and its
`_sqlx_migrations` receipt commit or roll back together.

This crate is a fork of sqlx-cloudflare-do; see the retained
[MIT attribution](LICENSE.sqlx-cloudflare). It is not published.

The driver refuses raw i64 arguments outside JavaScript's safe range. Dropping an
awaiting transaction future does not guarantee rollback on Worker.

Tests: see [tests/README.md](tests/README.md).
