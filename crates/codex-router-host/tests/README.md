# Host tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-router-host --features keychain-test-support,runtime-test-support
```

`integration/main.rs` gathers the 49 former external roots. Each root keeps its
module namespace. The cases cover Host startup and configuration, singleton
ownership, child process groups, update and restart lifecycles, enrollment,
collaboration composition, provider routes, wakes and native inventory.
Library unit tests remain colocated with their implementations.

The aggregate declares shared helper implementations once. Other directories
retain case expansions and fixture data. Externally consumed service helpers
keep their existing source owner; Host cases import the shared nested modules.
No empty e2e target is declared: this crate has no separately selected CI e2e
category.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

Relaunched test names derive from `module_path!()`. Normally completed children
with captured output must report one passed test. Intentional exits, kills,
exec replacements and managed children with discarded output retain actual
readiness, effect, exit-code or reaping checks that reject zero matches.
