# UW-15: Turso CTE metadata misclassifies generated JSON text

Status: in place, 2026-11-14. Tested engine/SDK: `turso 0.8.1`.
Adapter: `sqlx-turso 0.4.0`, agent-fleet-sync revision
`b75d072c116afa46cfa82facd34db6317f380687`.

## Evidence and boundary

The minimal query below produces runtime text (`typeof(payload) = 'text'`),
but its CTE result metadata describes the generated JSON column as `BLOB`.
The pinned adapter takes `turso::Column::decl_type()` as column type information
and applies non-null metadata to the returned value. A normal typed SQLx
`String` read therefore fails with `ColumnDecode`, while an unchecked diagnostic
read establishes that the underlying value is the exact expected JSON text.
This is metadata/strict decoding failure, not evidence that `json_object` returned
binary JSON. We have not patched either the engine or adapter.

```sql
WITH payloads AS (SELECT json_object('value',1) AS payload)
SELECT payload,typeof(payload) AS runtime_type FROM payloads;
```

The permanent reduced case
`signin_test_observation::tests::computed_json_text_cast_preserves_the_runtime_text_type_through_a_cte`
compares direct JSON, CTE JSON and the explicit-text projection. Observed on the
pinned versions, with a signed native test executable:

```text
direct: runtime=text, driver=TEXT, typed_text=true
cte:    runtime=text, driver=BLOB, typed_text=false
cast:   runtime=text, driver=TEXT, typed_text=true
```

Raw reproduction: `tmp/2026-11-14-signin-launcher/gates/upstream-reduction.log`,
1 passed / 0 failed, exit0. The prior full snapshot regression is preserved in
`core-tests-final.log`: 2 passed / 2 failed, exit100; both populated paths fail
at the strict String read before domain decoding.

The pinned source chain is Turso `src/lib.rs` `Statement::columns` →
`column_decltype`; adapter `sqlx-turso-core/src/column.rs` `from_turso` →
`executor/row_stream.rs` `with_type_info` → `value/mod.rs` metadata override.
The runtime diagnostic uses `typeof` and an unchecked text read only in this
minimal test. Product reads remain strict.

## Local correction

`collaboration-central-core/src/signin_test_observation.rs` projects
`CAST(payload AS TEXT) AS payload` from both snapshot CTEs. This types computed
JSON transport text; it does not coerce stored domain scalars. The query stays one
read-only statement with all rows, deterministic order and no cap.

Recreate from the repository root:

```sh
cargo nextest run --locked -p collaboration-central-core --features test-support --lib -E 'test(computed_json_text_cast_preserves)' --success-output final
```

## Removal check

On a Turso/adapter upgrade, run the reduced case and inspect all three reported
results. The uncast CTE must report TEXT and permit the normal typed String read,
with identical JSON bytes. Then remove both snapshot casts and require the five
snapshot tests, real native launcher child journey, and real signed Worker bridge
journey to pass. A successful unchecked read alone does not authorize removal.

The separate compound-SELECT grouping is intentional platform compatibility,
not an upstream defect: workerd's `setupSecurity` explicitly sets
`SQLITE_LIMIT_COMPOUND_SELECT` to five. Its source links SQLite's security-limit
rationale. The public Durable Objects limits page does not currently list this
specific cap; the authoritative implementation and observed six-term rejection
are the basis for the query comment. No defect report or dependency fork is added.

Sources: [workerd security limits](https://github.com/cloudflare/workerd/blob/main/src/workerd/util/sqlite.c++),
[SQLite compound SELECT limit](https://www.sqlite.org/limits.html#max_compound_select),
[Durable Objects limits](https://developers.cloudflare.com/durable-objects/platform/limits/).
