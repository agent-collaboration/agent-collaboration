# FleetRuntime test support

Consumers use the fixture to supply manual time and in-memory SQLite ports to
FleetRuntime tests. The adapter's own unit tests live in `fleet-store-sqlx`.

`src/signin_fixtures` supplies real signers, a loopback JWKS publisher and signed
person fixtures for both host suites. Business time comes from the caller's clock.

`integration/` opens the shared runtime and exercises alarm publication through
the fixture's real SQLite statement executor.
