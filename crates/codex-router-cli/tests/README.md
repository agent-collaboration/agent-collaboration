# Router CLI tests

Run with the feature set used by the main workspace CI job:

```sh
cargo nextest run --locked --profile ci -p codex-router-cli --features keychain-test-support
```

`integration/main.rs` gathers 12 roots covering account and recovery commands,
debug isolation and logging, compiled Host lifecycle, enrollment, proxy
overload, and the update guard that refuses `host` and account commands on a
root still needing `agent-router-update` (on a copy of the update tool's
`populated-root` fixture). `e2e/main.rs` retains the one ignored default-port
cutover case selected by its separate CI job. Ordinary package runs keep that
case ignored. Library and binary unit tests remain colocated.

Case expansions and fixture data retain their existing directories. The three
child selectors derive from `module_path!()`: the captured enrollment Host must
report one passed test, and managed native children retain real readiness, PID,
replacement and teardown checks that reject zero matches.

A former binary `<root>` and test `<case>` becomes binary `integration` or
`e2e` and test `<root>::<case>`. Unit-test identities do not change. Nextest
preserves one process per case, existing thread reservations and deadlines,
and the separate ignored-cutover job's original selection and profile.

The Claude parser unit uses `claude_acceptance_fixture` inside its retained
test root to avoid a repeated module name. Its body and leaf case name are
unchanged; the case mapping records that one namespace adjustment.
