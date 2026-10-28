# Retired Router FleetStore tests (B5 / decision 470)

These tests target the removed local authority. They are retired under the explicit B5 removal, not silently reported as ported native coverage. The cache/outbox tests remain and now read a real native Fleet producer.

## crates/message-board-storage/src/fleet_store_tests.rs

- `fleet_current_record_refuses_noncanonical_actor_metadata`
- `fleet_feed_refuses_noncanonical_body_text_without_changing_canonical_records`
- `native_fleet_replays_original_record_and_decision_after_rename_and_reopen`
- `shared_fleet_decision_tables_run_on_native_turso`
- `a_supplied_definition_id_collision_rolls_back_without_overwriting_content`

## crates/message-board-storage/src/fleet_receipt_tests.rs

- `thousand_board_archive_pages_replay_after_feed_pruning_without_later_receipt_append`
- `fleet_noop_receipt_keeps_original_position_and_scoped_cursor_refusals_after_pruning`

## crates/message-board-storage/src/fleet_archive_report_tests.rs

- `first_report_keeps_position_installation_and_source_time_after_reopen_and_successor_repeat`
- `a_finalizing_feed_constraint_rolls_report_and_both_archive_records_back`
- `malformed_automated_report_rows_never_count_as_successful_reports`


## Retained cases ported to the actual native Fleet owner

- Six cache projection/publication/rollback/reader-transaction cases moved from `fleet_cache_rebuild_tests.rs` to Tokio `fleet_cache_projection.rs`.
- Cache gap and repository-feed/reopen cases moved from `router_local_store_tests.rs` to Tokio `fleet_cache_records.rs`.
- Workspace identity/metadata reopen and two corrupt-name cases moved from the service's direct-authority tests to Tokio `fleet_metadata_records.rs`.
- `fleet_lifecycle_mcp.rs` keeps its three served-MCP lifecycle cases. Its snapshot, feed and ledger observations use the existing live machine session. Feed records, two-entry atomic finalization, request-ledger stability, original actor/origin, replay/noop and same-owner reopen assertions remain. The wire position is epoch/sequence; time bounds use the actual millisecond clock. Cache-facing tool reads synchronize after reporting.
- `request_effects_public.rs` keeps both public receipt cases. Its large receipt case prunes the actual central owner under the conformance feature, atomically preserves the feed floor, asserts deleted row count and zero retained rows, reopens the owner and reads the same signed public continuation.

The obsolete Router Fleet migrations and 29 SQLx entries were removed by the official native metadata regeneration, followed by the same `--check` workflow. No FleetStore/local admission implementation, migration seed, or old-authority fixture remains. `fleet.sqlite` appears only in negative assertions that the old database was not created. The separate LocalServer scenario port and later legacy core/SqlTx deletion remain distinct work.
