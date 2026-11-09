# Fleet SQL migrations on workerd

`public-migration-sql.ts` preserves seven cases from agent-fleet-sync b9470dc:
delivery fresh, v0.4.0 and typed-intermediate; lease fresh and v0.4.0; session
fresh and v0.4.0. It reads this repository's real production migration/query SQL
and retained v0.4.0 seed. Every original assertion remains, including namespace
isolation, exact legacy timestamps, typed custody/status projections and no
scratch UUID columns. It runs raw SQLite transactions in workerd, independently
of the 31 Rust adapter cases.

The journal fresh/v0.4.0 cases and the journal part of the native engine-key
audit remain obligations of agent-fleet-sync. They are not dropped or replaced
by these fleet-only receipts. No upstream test removal is authorized here.
