# Watch and subscription fidelity

This PR 2 slice implements the settled U79 correction at design adc51bce. The
reference is the actual ffced1c1 inbox and unread predicates, activate_watch,
activate_topic_watch_scope/activate_topic_root_watch, and record_subscription_post.

A plain project watch writes its own row. It supplies neither reply/state-change
coverage nor implicit thread rows. Project-subscription activation writes or
reactivates existing thread watches unless a thread-subscription override exists,
including an ended override. An already-active watch keeps its start. Later
posts create missing watches only for active, unexpired project subscribers,
without replacing disabled rows or overrides. Ending a membership leaves rows.

Both inbox directions, scalar and batch thread-watch reads, and watched thread
keysets use the actual thread row. Workspace unread summaries use the same inbox
predicate. The general ReaderState feed-metadata count contract is unchanged.
The existing storage/admission owners carry the correction; no migration,
replicated format, fleet directory, library or module-export change was made.

| Observation | Result |
|---|---|
| New native/admission regressions before product edits | 0 passed / 3 failed, exit 101: plain-watch reply/state leakage, missing existing-thread activation, and missing disabled-row reactivation |
| First corrected group | 3 / 0, exit 0 |
| Forward-only inbox fallback control | Compiled; 0 / 1, exit 101 at the unwatched reply/state exclusion |
| Backward-only inbox fallback control | Compiled; 0 / 1, exit 101: two latest records instead of the single selected main post |
| Old post watcher-selection control | Compiled; 0 / 1, exit 101: a plain watcher received a new-thread watch |
| Exact restoration and complete new group | 3 / 0, exit 0; source manifest unchanged |
| Retained covered-watch group | 4 / 0, exit 0 |
| Thirteen retained read/watch targets | First run 24 / 2, exit 101; the two obsolete fallback fixtures were corrected and their six cases passed, exit 0. The other eleven targets had 20 passing cases. Thus all 26 distinct cases passed across those runs. |
| Native SQL metadata regeneration and full prepare-sqlx-turso.py --check | Both exit 0 |
| Storage/service all-target Clippy, warnings denied | Exit 0 |

The two corrected retained fixtures preserve their original scope and corruption
oracles. bounded_thread_reads now establishes its watched keyset through actual
subscription activation. inbox_scope_isolation first proves that a plain foreign
project watch adds no replies or state changes in either direction, then activates
a project subscription, observes its thread row, and verifies later state changes
and retained latest reply history. No test was retired in this slice.

The three sensitivity controls restored exact correct source and metadata bytes
before the next operation; no baseline-only metadata remains. A subsequent type
refinement replaced open kind/id helper arguments with the existing closed
SubscriptionScope and was included in the restored green run. The initial test
attempt's String-versus-ThreadResolutionReason compile error was a fixture setup
failure, not a behavioral red.

Commands use the actual native writer and admission/application paths. Fixture
enrollment and local fleet policy remain declared inputs. These observations do
not prove central integration, two online Hosts, G2/G3/G4 or delivery acceptance.
The commands ran at HEAD 6f749a01 plus this slice's working-tree delta. Each
command's before/after source manifest matched. Only the two failing retained
target fixtures changed between those runs; their product sources were unchanged.
The final Clippy and metadata-check manifests cover the resulting commit's
3,857 non-proof source files. No fresh immutable export or whole-workspace
rerun is claimed for this slice.

The test/check commands (environment setup omitted) were:

```sh
cargo test -p collaboration-service --lib project_watch_fidelity --locked --offline
cargo test -p collaboration-service --lib covered_watches --locked --offline
cargo test -p collaboration-service --no-fail-fast --locked --offline \
  --test plain_watch_state --test watch_read_lifecycle --test inbox_lifecycle_mcp \
  --test inbox_scope_isolation --test inbox_summary_membership --test filtered_inboxes \
  --test inbox_cursor_contracts --test watch_history_guidance --test bounded_thread_reads \
  --test bounded_read_costs --test bounded_write_costs --test bounded_view_scope \
  --test reader_subject_metadata
cargo test -p collaboration-service --no-fail-fast --locked --offline \
  --test bounded_thread_reads --test inbox_scope_isolation
cargo clippy -p message-board-storage -p collaboration-service \
  --all-targets --locked --offline -- -D warnings
python3 scripts/tooling/prepare-sqlx-turso.py --check
cargo fmt --all -- --check
git diff --check
```

Each wrong build ran the named affected test alone through the same native test
command. Their expected failures are sensitivity evidence, not passing runs.
