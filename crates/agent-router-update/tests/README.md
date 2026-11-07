# Router update and board import tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p agent-router-update
```

`integration/main.rs` gathers 26 former external roots covering the `board-import`
subcommand: legacy transforms, identities, attribution and refusals, source and
seed integrity, message cutover, native inbox fidelity, publication and fleet
owner routes. Its `update_ledger_journey` root drives the real update binary
(J0) on a copy of `fixtures/populated-root`: refusal while the Host holds the
root, a dry run that changes no byte, kills at each step-0001 barrier followed
by a rerun, a never-connected root and an unexplained `fleet.sqlite`. Those
cases sign the binary with the debug runner before launching it.
`e2e/main.rs` retains the former `upgrade_binary_e2e` root, which drives the real
importer CLI through preparation, publication, replay and restart journeys.
Library and binary unit tests remain colocated.

Run only the serial CLI journeys after preparing the artifacts as CI does:

```sh
cargo nextest run --locked --profile ci -p agent-router-update --test e2e --test-threads 1
```

Each aggregate declares shared helper implementations once. The actual CLI
publication fixture lives in legacy-upgrade-test-support; package tests and
external consumers use that library through dev-dependencies. Case expansions
and fixture data stay in their existing directories.

A former binary `<root>` and test `<case>` becomes binary `integration` or
`e2e` and test `<root>::<case>`. Unit-test identities do not change. CI retains
the same main/e2e split, scheduling and deadlines; nextest uses one process per
case. The message-copy child selector derives from `module_path!()`. Its
intentional exit 93, durable copy and replay/drop checks reject zero matches.
