# codex-router-selection tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p codex-router-selection
```

`integration/main.rs` gathers 1 former external test root.
The external root checks eligibility and weighted selection properties, including boundary inputs. Library unit tests remain colocated with their implementations.

Each aggregate declares shared helper implementations once. Existing fixture
directories remain in place. Only nonempty test categories are declared in
Cargo.toml; CI selects no separate e2e category for this package.

A former binary `<root>` and test `<case>` becomes its category binary and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
