# Staging cutover tests

Unit tests are colocated with the Rust implementation in `src/*_test.rs`.
They cover transport reframing, unchanged row bytes, source cursor separation,
the existing Host singleton lock, private roots and retained source identities.

Run them through the repository signing runner with Cargo nextest:

```sh
cargo nextest run -p board-staging-cutover --profile ci
```

This in-progress tool has no integration or e2e category yet. Add each category
as a nonempty `tests/integration/main.rs` or `tests/e2e/main.rs` aggregate when
its first real-path test is implemented; no empty aggregate is declared.
