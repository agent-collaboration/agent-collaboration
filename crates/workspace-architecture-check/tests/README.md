# workspace-architecture-check tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p workspace-architecture-check
```

`integration/main.rs` gathers 2 former external test roots.
Cases enforce crate layers and the module-cycle ratchet; the tool-dependent cycle scan retains its existing ignored status. Library unit tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
