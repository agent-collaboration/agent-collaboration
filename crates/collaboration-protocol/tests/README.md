# Collaboration protocol tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p collaboration-protocol
```

`integration/main.rs` gathers the 33 former external roots. Cases cover wire
schemas, identity and mail conversions, provider and native execution contracts,
board notifications, delivery and run evidence, subscriptions, approvals,
questions and wake refusals. Library unit tests remain colocated.

Case expansions and fixture data retain their existing directories. There are
no shared helper implementations to hoist and no separately selected CI e2e
category, so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
