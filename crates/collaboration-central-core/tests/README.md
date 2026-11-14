# Central Core tests

Run with the conformance feature enabled in workspace CI:

```sh
cargo nextest run --locked --profile ci -p collaboration-central-core --features conformance,test-support
```

`integration/main.rs` gathers the 12 former external roots. Cases exercise
Central Core's identity, access, admission and wire contracts. Library unit
tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Case expansions,
contract fixtures and other data retain their existing directories. CI selects
no separate e2e category, so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

Sign-in policy unit cases retain B2's statement, key-cache, signature and replacement
assertions. New floor/admission cases check the static plans and row decoders; host
integration suites prove the private route, real cryptography and owning SQL batch.
The standalone unit selection needs `test-support` for its existing turn seams.

`fixtures/signin-observation-empty.json` is the canonical empty private launcher
observation used by Rust and the Node bridge. Colocated `signin_test_observation`
tests run the actual snapshot SQL over migrations, check the exact projection and
250-record completeness, reject corrupt stored domains, and verify no writes.
