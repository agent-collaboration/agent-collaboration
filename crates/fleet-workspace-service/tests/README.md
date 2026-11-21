# Workspace service tests

Colocated tests use the migrated workspace database to cover the retained status
lookup and its caller, cutoff and corruption decisions. The host suites prove
the actual typed transports.

The integration target migrates a fresh SQLite database and checks four
compile-time schema failures using the stock SQLx macro.
