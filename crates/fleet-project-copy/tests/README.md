# Tests

The library unit tests cover borrowed-scope copy rules, snapshots, installation,
identity validation and rollback against real Turso databases. The migrations
folder supplies their consumer schema. No standalone integration or e2e target
is declared.

The excluded standalone `workerd/` workspace owns real Durable Object copy,
certainty and FK adapter controls. Controllers and exact case identities live
in `verification/fixtures/fleet-copy-workerd`; native library passes do not
substitute for those 31 workerd cases.
