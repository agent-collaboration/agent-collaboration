# Fleet SQLx store tests

The colocated unit tests verify how SQLite rejection codes map to statement
rejections. The Tokio host's integration suite exercises the adapter through
the real central store and per-workspace stores.
