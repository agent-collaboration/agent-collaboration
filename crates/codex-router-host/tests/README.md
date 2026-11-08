# Host tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-router-host --features keychain-test-support,runtime-test-support
```

`integration/main.rs` gathers the external roots, including native model and skill bridge cases. Each root keeps its
module namespace. The cases cover Host startup and configuration, singleton
ownership, child process groups, update and restart lifecycles, enrollment,
collaboration composition, provider routes, wakes and native inventory.
Runtime replacement cases hold a real status probe across an app-server schema
change and require one Unavailable terminal for each cancelled operator request.
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

## Test folder ownership

- `integration/external_provider_supervisor`: integration modules, moved from `external_provider_supervisor/`.
- `integration/provider_acp_delivery_route`: integration modules, moved from `provider_acp_delivery_route/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
