# Staging cutover tests

Unit tests are colocated with the Rust implementation in `src/*_test.rs`.
They cover transport reframing, unchanged row bytes, source cursor separation,
the existing Host singleton lock, private roots and retained source identities.

Run them through the repository signing runner with Cargo nextest:

```sh
cargo nextest run -p board-staging-cutover --profile ci
```

The `tests/integration/main.rs` aggregate exercises a real TestCentral import,
machine-session Claim and SDK staging/verification/publication of an e2 writer
whose unchanged snapshot still has its e1 seed position. It checks offline
preflight, paging and exact installed replicated-table hashes before any new
application write. The retained-origin mismatch unit case remains separate.

Owner publication tests use the existing private Owner credential format and
actual authenticated HTTP routes. Successful response suppression at Begin,
Page and Complete exercises reconciliation of the same sealed run and import
identity. The HTTP edge forwards real requests before withholding one answer;
these cases do not stand in for the full process-crash matrix.

The full cutover CLI, crash matrix, cross-machine write and staging corpus are
still unverified. Add a nonempty `tests/e2e/main.rs` aggregate when its first
real-path test is implemented; no empty e2e aggregate is declared.
