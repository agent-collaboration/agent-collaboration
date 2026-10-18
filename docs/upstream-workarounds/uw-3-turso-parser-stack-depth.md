# UW-3: Turso's parser can overflow the default thread stack

**Status:** in place (decided and verified 2026-10-14)
**Lives in:** `crates/message-board-storage/src/project_reader_metadata.rs`

## In one paragraph

Pinned `turso_parser` 0.8.1 aborts a native debug process while parsing moderately nested SQL. Its expression-depth guard is 100, but recursive expression and subquery parsing can exhaust a default spawned-thread stack before reaching that guard. One combined reader-metadata query parsed every subject's nested predicates even for a thread reply. The service library's activation-boundary test aborted in the parser. Keeping one checked query per subject preserves the same results and lets that test complete without a stack or Cargo profile change.

## Upstream

- [Issue #6655: Stack overflow in arithmetic](https://github.com/tursodatabase/turso/issues/6655) and [PR #8097: fix stack overflow on moderately nested expressions](https://github.com/tursodatabase/turso/pull/8097) describe large debug frames exhausting the default stack before the expression guard. Both were open when checked on 2026-10-14; no released fix is inferred.
- [Issue #4900: very deeply nested subqueries](https://github.com/tursodatabase/turso/issues/4900) and [PR #9344: bound FROM/CTE query nesting depth](https://github.com/tursodatabase/turso/pull/9344) cover the related query-recursion path. They are related evidence, not proof that either patch fixes this recreation.
- The pinned crate's `src/parser.rs` uses recursive `parse_expr`, `parse_expr_operand`, expression-list and subquery parsing. Its own `MAX_EXPR_DEPTH` comment identifies debug stack use as the reason for a lower limit.

## Evidence

On this project's Apple Silicon Mac, the unchanged default-stack service test `activation_is_the_request_position_and_reactivation_changes_the_local_key` aborted reproducibly. LLDB stopped at an `EXC_BAD_ACCESS` during stack probing in `turso_parser::Parser::parse_expr_operand`. The bounded backtrace contained repeated `parse_expr_inner`, `parse_expr`, `parse_nexpr_list`, `parse_one_select` and `parse_select` frames. The query combined a CTE, all subject predicates, correlated `EXISTS` and JSON-function arguments.

A smaller parser-only recreation needs no database or application data. With the pinned native debug parser and `RUST_MIN_STACK` unset, the parenthesis-only sequence first aborted at depth 16; every depth from 1 through 15 completed. The process ended with `SIGABRT` at depth 16. The split-query service activation test passes on the same default stack.

## What we do

- Keep checked metadata statements shallow: separate Project, Thread, Task, Workstream and generic-reference queries behind one `unread_state_after` method.
- Preserve each subject predicate, exact unread count, newest 50 references, and the scanned range's dense-position/current-epoch checks.
- Remove the extra unread CTE wrapper. No record body decode, cache, summary table, new index, driver patch, stack-size setting or Cargo profile change is carried.
- Keep canonical and domain validation on content-body reads.

## How to recreate

Run the permanent [parser-only example](../../crates/sqlx-turso-core/examples/parser_stack_depth.rs) explicitly from the repository root:

```sh
cargo run --locked -p sqlx-turso-core --example parser_stack_depth -- 16
```

The example uses `std::thread::spawn` with its default stack and the existing pinned parser dependency. It can abort the process; it is a diagnostic executable, not a passing CI test. Its complete SQL is:

```sql
SELECT ((((((((((((((((1))))))))))))))));
```

For the measured adjacent passing case, use argument `15`. The smallest observed abort is specific to this build and host; other platforms or optimization levels may have a different threshold. Do not increase the stack to turn this check green.

## Remove or revisit when

On a Turso upgrade, rerun the default-stack recreation and the complete service-library activation and reader-metadata regressions. The parser must complete or return a bounded error without an engine abort, using a released depth-limited or iterative parser. Only then reconsider the query-depth workaround. A patch being open or merged is not that proof.
