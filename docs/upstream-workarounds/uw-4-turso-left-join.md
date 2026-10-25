# UW-4: Turso loses outer rows through a partial index after reopen

**Status:** in place, 2026-10-15
**Engine tested:** Turso 0.8.1

## Lives in

The workaround changes the current-record body batch. The pattern audit covers every `LEFT JOIN` in the three owning source trees (`*.rs`, `*.sql`, case insensitive, including multiline joins): 22 SQL joins in these 14 storage files. There are none in `automation-storage` or `collaboration-service`. A join-key comparison may use a value from the left side; the unsafe shape is a separate predicate that constrains only that side.

| Site and right alias | Disposition | Evidence or constraint |
|---|---|---|
| `project_record_batch.rs`, `t` | rewritten | Primary-key join; kind condition moved to `CASE` in selection. Reopened membership and mixed task/trail batches retain rows; Host restart preserves the healthy lead operand. |
| `project_feed_pages.rs`, `t` | proven | Reopened mixed task/trail feed batch returns both records, with immutable text retained and redaction applied in both include modes (`project_relation_tests`). Its left feed table has no partial index for trail rows. |
| `project_record_io.rs`, `t` | safe | Raw feed pages join the trail primary key alone and put the kind condition in `CASE`. Reopened redaction is retained by `project_relation_tests`; `bounded_window_reads` requires one SQL statement for the full trail page. |
| `project_subscription_reads.rs`, `p`, `w` | safe | Post scope uses the post primary key; batched watch probes use the reader/thread primary key. Neither `ON` has a separate left-side predicate. Native malformed-placement and window tests plus Host restart cover required rows and watch fallback. |
| `project_inbox_page.sql`, `w`, `m` | safe | Thread-watch and mark primary-key comparisons, including computed scope keys; no separate left-side predicate. |
| `project_inbox_page.sql`, `p` | proven | Its left-side thread-key condition is retained. `project_filter_keeps_global_watches_without_subscribing_to_project_threads` reopens before unread traversal, retaining main posts, foreign watched posts and state changes. |
| `project_inbox_page_backward.sql`, `w`, `m` | safe | Same primary-key comparisons as the forward page. |
| `project_inbox_page_backward.sql`, `p` | proven | The same reopened filtered-inbox test checks Latest results, retaining selected main posts and excluding unwatched roots. |
| `project_inbox_start.sql`, `m` | safe | Mark key is reader plus the watched thread's computed scope. The reopened unread traversal also exercises this query. |
| `project_view_card_intents.sql`, `cause` | safe | Both record-identity primary-key columns come from the intent's cause. |
| `project_view_card_intents.sql`, `thread`, `about_trail` | proven | `card_intent_reopen` queries project, task and workstream after reopen; two relevant post intents survive the direct-task and trail-about joins, an unrelated post intent is excluded from narrow subjects and retained for the project. |
| `project_membership_io.rs`, `r` | safe | Record-identity primary-key columns match the subscription binding; no separate left-side predicate. |
| `project_intent_io.rs`, `e` | safe | Writer epoch primary key equals the feed epoch. Ownership/range predicates are in `WHERE` or selection. |
| `project_thread_pages.rs`, `w`, `p` | safe | Thread-watch primary key and project-watch reader primary key; remaining `ON` predicates constrain the right table. |
| `fleet_discovery_all.sql`, `b` | safe | Board kind constrains the right alias; board ID is the parent key. |
| `fleet_discovery_workspace.sql`, `b` | safe | Same board primary-key lookup. |
| `fleet_discovery_board.sql`, `b` | safe | Same board primary-key lookup. |

The proven sites retain their SQL. Their tests establish the current schema/query combination; they do not establish that every outer predicate is safe under a different partial index. Recheck those tests when their indexes or joins change.

## Problem and evidence

The body batch used `LEFT JOIN work_trail_rows t ON r.kind = 'workTrailRow' AND t.row_id = r.id`. A non-trail record must survive with a null redaction value. After closing and reopening the database, the native engine instead omitted valid memberships and lead assignments. Direct primary-key reads still found them. This broke the two-project Host restart proof: the healthy project's next workstream write lost its lead operand.

The reduction needs only two tables, one row and a partial index on the left table. The original query returns the row before reopen and no rows afterward. Moving the kind condition into the selected value returns the row in both cases. The retained example calls the Turso SDK directly, without SQLx, board admission, capture or corruption injection. It prints both native query plans and both results.

## Recreate

Run the [SDK-only example](https://github.com/agent-collaboration/agent-fleet-sync/blob/c414fc5fab95bcffd3333f525b84f45cf997852b/crates/sqlx-turso-core/examples/left_join_outer_predicate.rs) from an agent-fleet-sync checkout at this workspace's pinned revision:

```sh
cargo run -p sqlx-turso-core --example left_join_outer_predicate
```

On the pinned engine this exits **1**, reporting `original=[]` and `corrected=["one"]` after reopen. The failure is intentional: a future engine that returns `["one"]` for both exits zero. The example retains the complete SQL and disposable database setup. Its data is synthetic.

## Workaround

Join the optional trail row by its primary key alone. Select `CASE WHEN r.kind = 'workTrailRow' THEN t.redacted END` for redaction. This preserves the nullable result for other kinds and keeps one indexed body batch. It adds no state, fallback, schema change or relaxed body validation.

Permanent product proofs cover reopened membership batches, mixed task/trail batches with an absent operand, retained redaction and immutable text, and healthy startup/write availability beside a corrupt pending feed body:

```sh
cargo test -p message-board-storage --lib membership
cargo test -p message-board-storage --lib project_relation_tests
cargo test -p codex-router-host --test board_failure_isolation
cargo test -p collaboration-service --test bounded_window_reads
cargo test -p collaboration-service --test card_intent_reopen
cargo test -p collaboration-service --test filtered_inboxes project_filter_keeps_global_watches_without_subscribing_to_project_threads
```

## Removal check

On an engine upgrade, require the direct SDK example to exit zero and return the expected row from both queries before and after reopen. Then test a candidate restoration of the original join against every product proof above and the bounded statement/decoded-record tests. Keep the workaround until all pass; a successful query execution without the expected rows is insufficient.
