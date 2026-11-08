# Board upgrade tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p board-legacy-upgrade
```

`integration/main.rs` gathers 26 former external roots covering legacy
transforms, identities, attribution and refusals, source and seed integrity,
message cutover, native inbox fidelity, publication and fleet owner routes.
`e2e/main.rs` retains the former `upgrade_binary_e2e` root, which drives the real
importer CLI through preparation, publication, replay and restart journeys.
Library and binary unit tests remain colocated.

Run only the serial CLI journeys after preparing the artifacts as CI does:

```sh
cargo nextest run --locked --profile ci -p board-legacy-upgrade --test e2e --test-threads 1
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

## Test folder ownership

- `integration/message_support`: integration modules, moved from `message_support/`.
- `e2e/upgrade_binary_e2e`: e2e modules, moved from `upgrade_binary_e2e/`.
- `support/upload_support`: e2e and integration modules, moved from `upload_support/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
