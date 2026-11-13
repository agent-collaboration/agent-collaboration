# Router CLI tests

Run with the feature set used by the main workspace CI job:

```sh
cargo nextest run --locked --profile ci -p codex-router-cli --features keychain-test-support
```

`integration/main.rs` covers account and recovery commands, debug isolation and
logging, compiled Host lifecycle, enrollment and proxy overload. The retired
root-updater guard and its dedicated journeys were removed. `e2e/main.rs` retains the one ignored default-port
cutover case selected by its separate CI job. Ordinary package runs keep that
case ignored. Library and binary unit tests remain colocated.

Case expansions and fixture data retain their existing directories. The three
child selectors derive from `module_path!()`: the captured enrollment Host must
report one passed test, and managed native children retain real readiness, PID,
replacement and teardown checks that reject zero matches.

Compiled Host installation fixtures sign and verify copied Router binaries before
launch. Their directory owner kills only processes rooted in that fixture (and
its recorded native children) on failure or drop. A real two-Serve regression
checks that failed-proof cleanup stops its child and preserves the neighboring
fixture's process.

A former binary `<root>` and test `<case>` becomes binary `integration` or
`e2e` and test `<root>::<case>`. Unit-test identities do not change. Nextest
preserves one process per case, existing thread reservations and deadlines,
and the separate ignored-cutover job's original selection and profile.

The Claude parser unit uses `claude_acceptance_fixture` inside its retained
test root to avoid a repeated module name. Its body and leaf case name are
unchanged; the case mapping records that one namespace adjustment.
