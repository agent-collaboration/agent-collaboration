# Collaboration link test support

Depend on this crate from `[dev-dependencies]`. The link transport and clock
run inside Turmoil hosts. The write simulation starts the production Router
link driver on the in-memory outbox and gives tests the Router's readbacks
and the simulated central request log. The existing Router write cases
remain in `collaboration-runtime/tests/integration/turmoil_link`.

`tokio_central::serve_tokio_central` serves the production `WorkspaceLink`
over Turmoil TCP. Pass it a provisioned `TokioWorkspaceHost` using the
simulation clock and connect with `TurmoilLinkTransport` at
`/v1/organizations/{organization}/link` and the fixture's machine token.
The central uses real SQLite worker threads. Use mark-bounded outcomes as
assertions because their I/O can complete at different simulation ticks.
Enable Tokio I/O on the Turmoil builder when the fixture serves sign-in keys
on loopback. The link itself still uses simulated TCP.
