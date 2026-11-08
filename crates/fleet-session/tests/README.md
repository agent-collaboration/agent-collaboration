# Tests

The integration target preserves every upstream public API and wire contract test.
`client_lifecycle` also covers capability negotiation and refusal of new batch
frames without Welcome support, using the real SessionClient and byte transport.
Private session-rule admission tests live beside `session_rules.rs`.
