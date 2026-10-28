# UW-3: Turso's parser can overflow the default thread stack

**Status:** in place (reader query split verified 2026-10-14; flat feed-range ownership checks verified 2026-10-19)
**Lives in:** `crates/message-board-storage/src/project_reader_metadata.rs` and `crates/message-board-storage/src/project_feed_pages.rs`

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

Run the permanent [parser-only example](https://github.com/agent-collaboration/agent-fleet-sync/blob/c414fc5fab95bcffd3333f525b84f45cf997852b/crates/sqlx-turso-core/examples/parser_stack_depth.rs) from an agent-fleet-sync checkout at this workspace's pinned revision:

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

## Feed-range aggregate: 2026-10-18

The same pinned parser defect also occurs in `ProjectSnapshot::validate_feed_range`, before the subject-specific reader statement. At pre-fix commit `6e5c99a7` with agent-fleet-sync `7f412db3`, the single membership-completion case below aborts with stack overflow, Cargo exit 101. The retained 114-frame crash trace contains recursive `turso_parser::Parser::parse_expr_inner`, `parse_expr`, `parse_where` and SELECT parsing, followed by native prepare, SQLx, `validate_feed_range` and `unread_state_after`. The failure happens during fixture project creation.

The original statement is:

```sql
SELECT COUNT(*) AS "count!: i64", COUNT(DISTINCT f.position) AS "distinct_count!: i64", COALESCE(SUM(CASE WHEN e.line_id IS NULL OR f.position<=e.inherited_through OR f.position>COALESCE((SELECT n.inherited_through FROM epoch_lines n WHERE n.epoch>e.epoch ORDER BY n.epoch LIMIT 1), ?) THEN 1 ELSE 0 END), 0) AS "invalid!: i64" FROM project_feed f LEFT JOIN epoch_lines e ON e.epoch=f.epoch WHERE f.position>? AND f.position<=?
```

Its next-epoch scalar SELECT is nested inside COALESCE, CASE, SUM and another COALESCE. We replace only the invalid-row aggregate with `COUNT(CASE WHEN <the same predicate> THEN 1 END)`. COUNT ignores the CASE's NULL for false or unknown predicates; both forms count exactly the true rows and return zero for an empty interval. The row count, distinct-position count, epoch ownership predicate, range and one-statement snapshot remain the same. This removes one parser expression level; no stack or Cargo profile setting is changed. Checked-query metadata outside the query/hash is identical.

The smallest observed real-path recreation uses the original statement at `6e5c99a7`, with `RUST_MIN_STACK` unset:

```sh
cargo test --locked --offline -p collaboration-service --lib \
  project_admission::behavior_tests::membership_completion::leave_replacement_and_expiry_end_rows_and_delete_windows \
  -- --exact --nocapture
```

That same case passes with the shallower aggregate. The native `feed_range_validation_accepts_valid_and_empty_ranges_but_counts_invalid_epochs` test in [project_native_integrity_tests.rs](../../crates/message-board-storage/src/project_native_integrity_tests.rs) separately requires valid and empty intervals to succeed and a row with a decodable but unregistered epoch to be refused. The existing reader-scope regressions retain gap and off-subject epoch checks. A focused pass does not establish the whole service suite.

On a Turso upgrade, the removal check must restore the original aggregate and regenerate checked metadata, then prepare it through the same default-stack membership path without an overflow. Require the native count guard and reader metadata regressions to pass as well. A larger stack or a passing parser-only preparation outside the failing call context does not satisfy that check.

## Thread unread predicate: 2026-10-19

At `33ec6fca`, the default-stack `project_subscription_keeps_off_activity_unread_and_thread_rows_take_precedence` test also aborts while preparing the Thread statement in `ProjectSnapshot::unread_state_after`. The independent debugger capture stops in `Lexer::next`, with recursive expression parsing, then `unread_state_after` at frame 67. Its callers are `reader_snapshot`, `reader_state`, `submit_at`, `create_root` and fixture startup. This call site is separate from the library's foreign-key orphan inspection failure tracked as W-C26/C24.

The [checked Thread query](../../crates/message-board-storage/src/project_thread_unread.sql) dispatches the same alternatives with `CASE record_kind`, and dispatches evaluator/lifecycle facts with their kind discriminator. Linked facts still match independently of their kind. Thread, refile, post, fact, intent and accepted-request conditions remain in one statement, with the same bounds, `COUNT(*) OVER()`, descending position order and `LIMIT 50`. There is no schema change, additional query or stack setting.

The permanent [equivalence test](../../crates/message-board-storage/src/project_thread_unread_tests.rs) compares that query with the [original predicate](../../crates/message-board-storage/tests/fixtures/thread_unread_original.sql) on the same SQLite data. Positive and negative cases cover every alternative, including each fact route, a fact matching two routes, bounded intervals and more than fifty hits. SQLite establishes predicate meaning here; it does not establish native parser depth. The native recreation is:

```sh
cargo test --locked --offline -p collaboration-service --lib \
  project_admission::behavior_tests::project_coverage::project_subscription_keeps_off_activity_unread_and_thread_rows_take_precedence \
  -- --exact --nocapture
```

The rewritten statement passes this exact native path with `RUST_MIN_STACK` unset. A control that omits the lifecycle Thread fact predicate makes the equivalence test fail (71 versus 72 matches); the production CASE query and checked metadata are restored byte for byte after that control. On a Turso upgrade, restore the original Thread predicate and regenerate its checked metadata, then require this exact native path to prepare without overflow on the default stack. Require the equivalence test and all storage/service test targets as well. A passing preparation in a shallower caller, the separate feed-range fix, or a larger stack does not satisfy this removal check.


## Feed-range ownership bounds at the deeper creating call: 2026-10-19

The COUNT(CASE ...) form above still overflows on the default stack at library `13816305`, in `membership_lifecycle::unsubscribe_keeps_watch_but_unwatch_ends_without_post_reactivation`. Both that exact test and the full service library abort. A complete 95-frame LLDB trace shows repeated expression/operand/select parsing, then `StatementGuard::check_read` and the board's `ProjectSnapshot::validate_feed_range`. The path is fixture creation -> create_project_reply -> project_reader_state -> reader_snapshot -> unread_state_after -> validate_feed_range. This is the library parsing consumer SQL, not its generated orphan check. The exact overflowing SQL is the inline range aggregate containing a successor-epoch SELECT beneath COALESCE, CASE and COUNT. A separately authorized 8 MiB diagnostic run passes once; it is not a gate or a carried setting.

The range guard now uses shallow checked statements in the same native read transaction:

1. Count rows and globally distinct positions over the requested interval, retaining gap and cross-epoch duplicate refusal.
2. Select the first and last position and owning line per epoch represented in that interval. The LEFT JOIN is only `e.epoch=f.epoch`, respecting UW-4; no outer-only predicate is added to ON.
3. For each such epoch, perform one indexed `epoch>? ORDER BY epoch LIMIT 1` successor lookup. Its inherited position is the upper bound; a missing successor uses the requested interval end. Reject a missing owner, a first position at/below the owner's inherited position, or a last position above the successor bound.

Min/max establish the same per-row ownership predicate without a scalar subquery inside an aggregate. All statements share the already pinned SQLx/native transaction, so no multi-statement snapshot is inferred from independent reads. The tradeoff is up to 2+E metadata statements for E represented epochs, including a second range scan, instead of one compound statement. No unrelated epoch history or record body is transferred, and no new schema, state, index, stack or profile setting is introduced.

The exact membership regression passes with RUST_MIN_STACK unset. The native integrity target passes all four tests. Its new sparse-epoch case covers epochs 1/3/8; valid, empty and partial ranges; invalid and superseded cursors; both inherited bounds; a missing owner; duplicate positions; and a position-zero resume. A wrong build changing the inherited-bound rejection's OR to AND fails that case at epoch 1, position 3 (0/1, exit 101). Production is restored byte for byte before completion gates. Full service/storage suite receipts are recorded in the work trace; the focused pass alone is not a whole-suite claim.

The complete default-stack service library then passes 374/0 with nine pre-existing ignored tests, and storage passes 67/0 with one pre-existing ignored child fixture. Thirteen focused native view, reader-scope and read-cost cases also pass. This establishes these component gates; the full integration collection and composed messaging delivery remain separate obligations.

The bounded literal/file SQL audit inspects 2,277 source statements and finds six remaining SELECT-under-CASE/COALESCE candidates. Depth means SQL parenthesis nesting at the SELECT, not native parser frame depth; it is a discovery measure, not a safety threshold. Dynamic SQL assembly is outside that literal scan.

| Site | SELECT depth; maximum query depth | Disposition |
|---|---|---|
| automation_event_history.rs event high-water | 2; 2 | SQLite automation owner; does not use the Turso read guard. Retained. |
| wakeup_repository.rs current wake high-water | 2; 2 | SQLite automation owner. Retained. |
| project_thread_unread.sql | 1 beneath CASE; 3 | Existing subject-specific UW-3 query; covered by native membership/reader paths and its meaning comparison. Retained unless a real default-stack call overflows. |
| project_view_card_intents.sql | 2 beneath CASE; 5 | Current task/workstream projection; retains key-only joins. Existing card/native view paths are its proof. No overflow established by the static scan. |
| project_view_pending_decisions.sql | 1 and 2 beneath CASE; 5 | Current pending-decision projection. No overflow established by the static scan. |
| project_view_reads.rs move cutoff | 2 beneath COALESCE; 3 | Current move projection. No overflow established by the static scan. |

On a Turso/parser upgrade, restore the original one-statement range predicate and regenerate checked metadata, then require the exact default-stack membership call, full service library and the sparse-epoch integrity/reader regressions to pass. Also compare native read costs before removing this split. A parser-only preparation, a larger-stack pass or C24's separate orphan-query proof cannot establish that removal check.
