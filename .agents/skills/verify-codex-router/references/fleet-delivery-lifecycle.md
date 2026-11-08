# Delivery lifecycle through seven exact native identities

This journey preserves the upstream `b9470dc` delivery-lifecycle driver's seven
real Turso/engine cases in their owning `fleet-mail-delivery` package. The shell
entry calls `verification/tooling/run-fleet-delivery-lifecycle.ts`; its adjacent
contract owns the exact selected identities and rejects incomplete discovery
or execution. Full-package success alone is insufficient.

The caller supplies declared source/apparatus, an exclusive normal-target
window and a fresh owned evidence path. From the repository root:

```sh
bash scripts/verification/delivery-lifecycle.sh --help
bash scripts/verification/delivery-lifecycle.sh "$EVIDENCE/delivery"
```

The argument directory must not exist. The driver checks at least 30 GiB free,
uses normal `target/`, removes `GITHUB_ACTIONS` from its local child environment
and refuses runner overrides. `cargo nextest list` discovers the library and
integration selections with `--all-features`. The configured signing runner
handles discovery; on Darwin the driver additionally pre-signs each discovered
normal-target executable and verifies its strict signature and expected team
`974QD84WVC` before running tests. It introduces no signing-policy exemption.

## Exact contract and deciding assertions

| Identity                                                                                             | Deciding real-path observation                                                                                                             |
| ---------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `relay_schema::tests::relay_retirement_preserves_relay_api_calls_across_reopen`                      | Retired relay tables stay absent through real API calls and owning store reopen; retained relay functions and eventual removal still work. |
| `relay_schema::tests::relay_retirement_is_transactional_and_leaves_other_tables_unchanged`           | Caller rollback keeps ten retired tables; successful retirement preserves nine other tables.                                               |
| `relay_schema::tests::relay_retirement_accepts_a_store_with_no_delivery_tables`                      | Retirement twice on a journal-only store leaves no delivery tables.                                                                        |
| `relay_schema::tests::machine_startup_preserves_custody_bytes_and_removal_still_cleans_both_homes`   | Startup/reopen retains exact custody/cursor rows, keeps Machine tables and later removes both homes while preserving the cursor.           |
| `local_cancel_routes::push_promotes_a_prevented_local_original_and_reports_once_across_reopen`       | Push owes one unchanged prevented report; reopen and replay retain the same snapshot without another report.                               |
| `local_cancel_routes::failed_preserved_outcome_projection_rolls_back_push_promotion_and_report_debt` | Real projection refusal rolls back the transaction to the exact prior snapshot and commits no projection.                                  |
| `relay_outbox::unadmitted_cancel_reports_return_typed_refusal_without_any_transaction_write`         | Prevented/TooLate reports return the typed refusal with unchanged transaction total_changes; backend failure remains a backend error.      |

The first four run in the library test binary, defined in
`src/relay_schema_test.rs`; the final three run in the integration aggregate's
`local_cancel_routes` and `relay_outbox` modules. A same-named case in a
different package/binary does not satisfy this contract.

Discovery must select exactly these seven, with none ignored. Signed nextest
runs them with one process per test and one test at a time. The final result
checker requires each exact binary/name pair to finish once as PASS; missing,
skipped, duplicate or failed required results and unexpected executed cases
prevent `complete.json`. Filtered unrelated cases may remain skipped. A zero
match or successful empty package banner never counts as coverage.

Return `complete.json`, `discovery.log`, `execution.log`, child commands/exits,
before/after source state, artifact hashes and signature/team checks. The driver
requires unchanged source through execution and exits nonzero on incomplete
observation. Keep its failure file and raw logs instead of reporting an
assertion pass when compilation, signing or discovery stopped the route.

The underlying fixtures own their isolated stores/engines; confirm the command
and owners settled. Preserve evidence outside `target/`, then clean only owned
residue after other consumers release the normal build output. Contract checker
tests run under the existing verification tooling test glob; changing result
parsing requires its missing/skipped/duplicate/wrong-binary negative controls.
These are native library/integration observations, with no hosted delivery,
provider, production process, release or acceptance claim.
