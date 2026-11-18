# Central rip-out, item 9: the Router tests that ran on the old central

Owner decision (2026-11-17): "Yes remove old and make sure you craft new tests that test behaviours with testing and Hegel and turmoil". The replacement tests come in PR 3 and PR 4.

**Rule (Lead A, from the owner's A):** a test dies with its code, not with its fixture. A test whose assertions do not need the old central stays, on a fixture with the old-central setup taken out; it is deleted later, together with the code it tests. A test stays deleted here only when it needs the old central: to start it, to hold a session with it, or to set up state that only the old central can register.

| | Tests |
|---|---|
| Kept: no old-central dependency (restored on a central-free fixture) | 88 |
| Deleted: needs the old central | 630 |
| **Considered** | **718** |

## How the kept tests run now

- **Host startup (F1):** `startup_contract_tests`'s fixture is a local, unenrolled Host. Its enrolled branch, which started `board_central_test_support::TestCentral`, is removed, and so are the two enrolled cases.
- **Local mail (F2):** `mail_push_delivery_fixture`'s `FleetClient` has no session. The `machine_session` call and its central synchronization are removed, and the local mail runtime is unchanged.
- **Overload (F3):** the collaboration-client `SaturatedApi` and the CLI shed case build a session-less local `FleetClient`.
- **Default listener (F3):** the MCP default-listener cases use the central-free wake application.

## Deleted, by reason

| Reason | Tests |
|---|---|
| runs on BoardFixture's old-central board (enrolled Host, central-backed board) | 237 |
| OwnerFixture registers its workspace, board and project through the old central's session | 171 |
| SubscriptionFixture registers its project through the old central's session | 78 |
| starts the old central (board-central-test-support) for its setup or calls | 60 |
| process journey on the old central pair or a central-backed board (board_e2e fixtures) | 44 |
| enrolled machine session against the old central (board composition fixtures) | 16 |
| central-backed board fixture | 10 |
| runtime roots with a central-backed board setup | 6 |
| needs a real board through the old central (board_application); carried into PR 4's proof plan | 4 |
| enrolled Host: needs the old central's machine session | 3 |
| tests the removed old-central fixture crate itself | 1 |

The OwnerFixture and SubscriptionFixture cases create their workspace, board and registered project through the old central's session, then stop it before the tested behavior runs. Keeping them would need a stand-in that seeds that state without the old central. That stand-in was the alternative to deletion, and it is open with Lead A.

## collaboration-mcp `collaboration_api_router_tests/call_lifecycle.rs`, every case

| Case | Status | Reason |
|---|---|---|
| `a_client_that_disconnects_cancels_its_call_on_both_listeners` | kept | no old-central dependency; cancellation at the listener |
| `a_wake_wait_ends_at_its_bound_while_the_store_is_held` | kept | no old-central dependency |
| `a_client_that_disconnects_from_a_mutation_waiting_on_the_store_leaves_no_mutation` | deleted | needs a real board through the old central (board_application); carried into PR 4's proof plan |

## Kept tests

| Package | File | Test |
|---|---|---|
| agent-collaboration | `tests/integration/admission_overload_cli.rs` | `shed_commands_report_an_overload_to_retry` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests.rs` | `whole_host_shutdown_retains_the_native_failure_when_link_observation_fails` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/broker_context_tests.rs` | `host_broker_notices_use_the_shared_display_names_and_physical_push_store` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/observation_recheck_tests.rs` | `a_host_with_a_journal_runs_no_observation_rechecks` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/observation_recheck_tests.rs` | `a_journal_that_failed_to_open_is_replaced_by_observation_rechecks` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/scheduler_completion_tests.rs` | `the_hosts_scheduler_wakes_on_each_completion_source` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/status_cancellation_test.rs` | `cancelling_a_pending_fleet_read_releases_store_ownership_before_runtime_cleanup` |
| codex-router-host | `src/collaboration_runtime/startup_contract_tests/wake_capacity_tests.rs` | `host_inbound_wake_waits_exhaust_outbound_wait_capacity` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_board_call_is_a_typed_retryable_board_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_board_write_is_not_an_unknown_outcome` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_configuration_call_is_a_typed_retryable_configuration_failure` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_first_fire_wait_can_be_started_again` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_inspection_call_is_a_typed_retryable_inspection_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_instruction_call_is_a_typed_retryable_instruction_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_run_call_is_a_typed_retryable_run_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_schedule_call_is_a_typed_retryable_schedule_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_untyped_call_reports_no_effect_and_retry` |
| collaboration-client | `tests/integration/admission_overload.rs` | `a_shed_wake_call_is_a_typed_retryable_wake_overload` |
| collaboration-client | `tests/integration/admission_overload.rs` | `shed_native_discovery_keeps_the_overload_for_the_bridge_and_the_launcher` |
| collaboration-mcp | `src/collaboration_api_router_tests.rs` | `a_foreign_browser_origin_is_refused_before_dispatch` |
| collaboration-mcp | `src/collaboration_api_router_tests.rs` | `the_api_serves_on_ipv6_loopback` |
| collaboration-service | `src/collaboration_application/message_status_operations_test.rs` | `missing_sender_status_at_day59_is_unknown` |
| collaboration-service | `src/collaboration_application/message_status_operations_test.rs` | `missing_sender_status_at_day61_says_removed_after60_days` |
| collaboration-service | `src/frozen_cancel_settlement_tests.rs` | `a_frozen_local_child_rederives_the_earlier_control_after_reopen_without_false_custody` |
| collaboration-service | `src/frozen_remote_cancel_tests.rs` | `a_relay_refused_proposed_child_keeps_its_epoch_but_waits_for_the_other_control` |
| collaboration-service | `src/frozen_remote_cancel_tests.rs` | `a_remote_pre_custody_duplicate_queries_the_canonical_control_and_target_after_reopen` |
| collaboration-service | `src/frozen_remote_cancel_tests.rs` | `project_frontier_release_preserves_canonical_controls_without_early_custody_retirement` |
| collaboration-service | `src/local_cancel_crash_tests.rs` | `crash_between_attribution_and_sdk_admission_leaves_neither_after_reopen` |
| collaboration-service | `src/local_cancel_crash_tests.rs` | `local_cancel_crash_child_entry` |
| collaboration-service | `src/local_cancel_report_tests.rs` | `a_final_local_push_promotion_reports_the_preserved_outcome_once_across_replay` |
| collaboration-service | `src/local_cancel_report_tests.rs` | `local_cancel_journals_the_report_for_an_actual_relayed_inbox_original` |
| collaboration-service | `src/local_mail_admission_tests.rs` | `a_full_real_inbox_refuses_new_projections_but_reserves_space_to_cancel_its_original` |
| collaboration-service | `src/local_mail_admission_tests.rs` | `overlapping_same_id_sends_never_repeat_the_native_attempt_and_later_replay_is_stable` |
| collaboration-service | `src/local_mail_admission_tests.rs` | `settled_local_answer_replays_inside_two_months_without_resurrecting_a_push_or_call` |
| collaboration-service | `src/local_mail_interruption_tests.rs` | `interrupted_sdk_dispatch_reopens_as_uncertain_without_repeating_the_native_call` |
| collaboration-service | `src/local_mail_readiness_tests.rs` | `readiness_release_then_unload_does_not_load_or_submit_a_held_auto_again` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `canonical_cancel_keeps_the_first_requester_and_removes_only_the_new_tentative_row` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `failed_control_projection_rolls_back_the_local_record_and_sdk_admission_together` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `failed_prevented_projection_rolls_back_the_original_but_keeps_the_pending_cancel` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `late_local_intake_records_the_control_outcome_and_only_marks_the_delivered_target` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `local_intake_prepares_attribution_before_the_original_is_prevented_without_native_io` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `remote_canonical_cancel_returns_unknown_local_attribution_without_inventing_a_row` |
| collaboration-service | `src/local_mail_runtime_tests.rs` | `sender_mismatch_refuses_only_the_local_control_and_delivers_the_original_once` |
| collaboration-service | `src/mail_delivery_runner_probe.rs` | `a_blocked_native_reader_does_not_block_another_due_mail_item` |
| collaboration-service | `src/mail_delivery_runtime_retention_test.rs` | `local_custody_survives_project_frontier_release_and_day59_then_removes_at_day60` |
| collaboration-service | `src/mail_delivery_runtime_retention_test.rs` | `public_message_send_preserves_too_old_without_custody_or_a_native_attempt` |
| collaboration-service | `src/mail_delivery_runtime_retention_test.rs` | `sender_terminal_custody_has_no_early_retire_task_and_expires_only_at_day60` |
| collaboration-service | `src/mail_push_delivery_tests.rs` | `poll_has_no_push_or_native_call_at_the_delivery_entry` |
| collaboration-service | `src/mail_push_delivery_tests.rs` | `remote_follow_retries_use_item_counts_and_library_deadlines_across_cold_reopen` |
| collaboration-service | `src/mail_push_delivery_tests.rs` | `retry_counter_failure_rolls_back_outcome_and_saved_native_answer_does_not_resend` |
| collaboration-service | `src/mail_push_delivery_tests.rs` | `retry_counts_validate_stored_ranges_and_saturate_without_changing_identity` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `coded_direct_outcome_rollback_recovers_saved_answer_without_a_second_native_call` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `cold_replay_after_attempt_mark_holds_unloaded_auto_without_native_submission` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `direct_defects_are_terminal_but_follow_defects_keep_item_backoff_and_full_receipts` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `direct_rejection_table_reaches_real_sdk_custody_and_native_projection_after_reopen` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `holdable_native_answer_obeys_frozen_steer_and_guard_without_losing_its_receipt` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `native_no_route_normalization_precedes_canonical_refusal_and_respects_the_guard` |
| collaboration-service | `src/mail_push_direct_delivery_tests.rs` | `unloaded_direct_policy_holds_or_returns_coded_refusal_without_native_submission` |
| collaboration-service | `src/mail_removal_backfill_test.rs` | `backfill_process_child` |
| collaboration-service | `src/mail_removal_backfill_test.rs` | `completed_backfill_skips_the_sdk_custody_scan` |
| collaboration-service | `src/mail_removal_backfill_test.rs` | `corrupt_completion_marker_refuses_startup_instead_of_skipping` |
| collaboration-service | `src/mail_removal_backfill_test.rs` | `sigkill_before_backfill_commit_retries_both_marker_and_admissions` |
| collaboration-service | `src/mail_removal_backfill_test.rs` | `startup_backfills_legacy_custody_once_and_retains_it_until_day60` |
| collaboration-service | `src/mail_submitter_backfill_test.rs` | `startup_backfills_legacy_submitter_once_before_driving_mail` |
| collaboration-service | `src/project_mail_admission_tests.rs` | `a_fresh_successor_admission_records_its_own_epoch_without_waiting_for_welcome` |
| collaboration-service | `src/project_mail_admission_tests.rs` | `project_admission_keeps_the_original_epoch_on_reholding_and_sdk_conflict_changes_nothing` |
| collaboration-service | `src/project_mail_admission_tests.rs` | `unconfigured_project_admission_is_unavailable_without_custody_or_epoch_after_reopen` |
| collaboration-service | `src/project_mail_admission_tests.rs` | `unconfigured_remote_direct_send_is_unavailable_without_native_admission` |
| collaboration-service | `src/project_mail_cancel_admission_tests.rs` | `a_relay_cancel_refusal_remains_another_canonical_control_on_same_id_replay` |
| collaboration-service | `src/project_mail_cancel_admission_tests.rs` | `another_canonical_cancel_leaves_no_proposed_child_custody_or_epoch_after_reopen` |
| collaboration-service | `src/project_mail_cancel_admission_tests.rs` | `unconfigured_project_cancel_returns_unavailable_without_custody_or_epoch` |
| collaboration-service | `src/project_mail_crash_tests.rs` | `project_outbox_crash_child_entry` |
| collaboration-service | `src/project_mail_crash_tests.rs` | `sigkill_between_cancel_custody_and_epoch_writes_preserves_neither_after_reopen` |
| collaboration-service | `src/project_mail_crash_tests.rs` | `sigkill_between_sdk_and_epoch_writes_preserves_neither_after_owning_reopen` |
| collaboration-service | `src/remote_direct_send_tests.rs` | `configured_application_send_queues_offline_replays_and_rejects_conflicting_content_without_a_push` |
| collaboration-service | `src/remote_direct_send_tests.rs` | `unconfigured_remote_application_send_has_no_custody_answer_or_epoch` |
| collaboration-service | `src/remote_mail_assembly_tests.rs` | `local_send_completes_while_the_attached_remote_session_is_unwelcomed_and_closed` |
| collaboration-service | `src/remote_mail_assembly_tests.rs` | `mismatched_session_peer_refuses_assembly_before_any_mail_is_admitted` |
| collaboration-service | `src/remote_message_read_tests.rs` | `public_history_order_merges_local_and_remote_before_applying_the_shared_limit` |
| collaboration-service | `src/remote_message_read_tests.rs` | `public_history_order_uses_admission_time_when_supplied_uuid_order_is_inverted` |
| collaboration-service | `src/remote_message_read_tests.rs` | `remote_show_and_history_check_immutable_sender_context_before_canonical_status_after_reopen` |
| collaboration-service | `src/remote_wake_replay_tests.rs` | `wake_operations_remote_aged_control_keeps_replay_after_retention` |
| collaboration-service | `src/remote_wake_replay_tests.rs` | `wake_operations_remote_creation_late_replay_returns_recorded_result` |
| collaboration-service | `src/remote_wake_replay_tests.rs` | `wake_operations_remote_expired_new_request_has_no_custody` |
| collaboration-service | `src/remote_wake_replay_tests.rs` | `wake_operations_remote_lost_result_after_deadline_remains_unknown` |
| collaboration-service | `src/remote_wake_replay_tests.rs` | `wake_operations_remote_pause_late_replay_returns_recorded_result` |
| collaboration-service | `src/remote_wake_wait_tests.rs` | `a_remote_wake_wait_answers_the_status_commit_that_lands_after_its_empty_read` |
| collaboration-service | `src/remote_wake_wait_tests.rs` | `a_remote_wake_wait_with_nothing_committed_reads_once_then_once_at_its_deadline` |

## Deleted tests

### agent-collaboration (65)

| File | Test | Reason |
|---|---|---|
| `tests/delivery_matrix/owned_resource_cleanup_producer_runtime_tests.rs` | `actual_two_root_producer_uses_distinct_boards_and_preserves_participant_acl` | runtime roots with a central-backed board setup |
| `tests/delivery_matrix/owned_resource_cleanup_runtime_tests.rs` | `runtime_enabled_peer_schedule_becomes_disabled` | runtime roots with a central-backed board setup |
| `tests/delivery_matrix/owned_resource_cleanup_runtime_tests.rs` | `runtime_error_after_wake_receipt_retires_known_prefix` | runtime roots with a central-backed board setup |
| `tests/delivery_matrix/owned_resource_cleanup_runtime_tests.rs` | `runtime_first_reader_is_retired_before_second_root_exists` | runtime roots with a central-backed board setup |
| `tests/delivery_matrix/owned_resource_cleanup_runtime_tests.rs` | `runtime_schedule_before_prepare_is_cleanup_owned` | runtime roots with a central-backed board setup |
| `tests/delivery_matrix/owned_resource_cleanup_runtime_tests.rs` | `stopped_runtime_keeps_all_cleanup_attempts_unknown_without_retry` | runtime roots with a central-backed board setup |
| `tests/e2e/board_central_e2e.rs` | `bc01_replicated_prefix_mapping_replay_and_owning_reopen` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_central_e2e.rs` | `bc02_move_and_takeover_install_exact_native_authority` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_central_e2e.rs` | `bc03_archive_notices_settle_and_copied_content_stays_readable` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_central_e2e.rs` | `bc04_forwarding_uses_verified_origin_and_replays_one_effect` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_central_e2e.rs` | `bc06_committed_lease_expiry_changes_no_version_or_reader_copy` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `a_closed_claude_peer_follow_is_refused_at_once_and_a_later_post_reaches_it_running` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `aggregate_cursor_keeps_unavailable_member_positions_until_recovery` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `all_thread_subjects_refile_replay_and_current_titles_survive_reopen` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `complete_posts_participants_and_threads_cross_both_listeners_and_survive_corrupt_sibling` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `corrupt_membership_keeps_the_same_readers_healthy_project_and_owed_poll` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `duplicate_semantic_ack_refuses_before_either_request_key_or_unread_coverage` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `each_live_reviewer_receives_its_own_frozen_notification_before_and_after_replacement` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `foreign_thread_watch_keeps_context_while_plain_project_watch_adds_no_coverage` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `held_follow_payload_outlives_real_terminal_history_pruning_and_delivers_original_range_once` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `keyed_work_and_causal_late_facts_survive_reopen` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `pending_views_settle_revive_and_isolate_foreign_metadata_without_marking_reads` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `poll_ranges_settle_once_and_keep_policy_identity_and_owed_work_on_reopen` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `real_host_central_and_cli_mcp_share_the_persisted_installation` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `rejected_native_scopes_never_enter_healthy_aggregate_reads_or_session_context` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `repository_default_lists_preserve_current_subjects_and_hide_redacted_or_missing_sources` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `role_authority_cardinality_and_history_survive_owning_reopen` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_records_e2e.rs` | `unavailable_workspace_sibling_preserves_healthy_rows_and_explicit_refusal` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `held_local_follow_stays_prevented_when_takeover_discards_its_subscription_end` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `lost_cancel_receipt_replays_the_same_cancel_after_takeover_without_a_recipient_effect` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `owed_follow_survives_clean_move_with_the_same_item_generation_and_range` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `owed_follow_survives_takeover_with_the_same_item_generation_and_range` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `replicated_subscription_end_reuses_its_original_cancel_and_starts_no_wake` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `settled_subscription_end_keeps_its_frozen_cancel_without_local_replay_or_a_wake` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `takeover_before_local_cause_capture_does_not_invent_remote_custody` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `takeover_recovers_the_same_frozen_item_after_holder_dies_with_both_commits_acknowledged` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `takeover_with_neither_commit_acknowledged_does_not_invent_remote_custody` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `takeover_with_only_cause_acknowledged_derives_a_new_item_once` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/board_takeover_e2e.rs` | `unreplicated_subscription_end_cannot_cancel_the_successor_follow` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/installation_directory_e2e.rs` | `welcome_refresh_queues_a_never_looked_up_recipient_after_central_stops` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/mail_reconcile_e2e.rs` | `connected_timer_repairs_lost_live_status_without_reconnect_or_another_send` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/mail_retirement_e2e.rs` | `delivered_direct_mail_keeps_terminal_custody_and_replay_after_host_restart` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/mail_retirement_e2e.rs` | `remote_wake_requests_and_mutations_keep_terminal_custody` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/mail_retirement_e2e.rs` | `terminal_direct_mail_retains_custody_while_later_items_deliver` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `full_inbox_retries_frozen_items_after_recipient_capacity_is_restored` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `handoff_commit_before_outbox_write_recovers_the_same_item` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `lost_carrier_receipt_stays_uncertain` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `mailbox_hop_duplicate_retry_keeps_one_recipient_effect` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `same_item_identity_with_changed_content_is_a_conflict` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/e2e/messaging_rules_e2e.rs` | `valid_until_expiry_is_visible_to_the_sender` | process journey on the old central pair or a central-backed board (board_e2e fixtures) |
| `tests/integration/board_aggregate_cli.rs` | `aggregate_cli_pages_and_batch_acknowledgements_keep_project_keys` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/board_fleet_lifecycle_cli.rs` | `initiative_close_and_workspace_archive_reach_the_fleet_owner` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_model_cli.rs` | `renamed_command_groups_execute_real_board_journey_and_keep_caller_request_ids` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/board_request_effects_cli.rs` | `request_effects_cli_reads_a_decision_and_transmits_an_after_argument` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_request_effects_cli.rs` | `request_effects_cli_resumes_a_signed_page_without_normalizing_the_token` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_task_corrections_cli.rs` | `drop_reason_and_removed_list_flags_reach_the_real_board` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_views_cli.rs` | `view_commands_reach_the_real_service_and_report_read_marks` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_repository_subjects.rs` | `repository_and_default_lists_follow_subject_rename_and_refile` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_repository_subjects.rs` | `repository_and_default_lists_keep_redacted_subjects_private` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_repository_subjects.rs` | `repository_and_default_lists_keep_unavailable_subjects_and_imported_marker` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_surface_parity.rs` | `every_about_grammar_and_refile_match_cli_mcp_and_current_subject_reads` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_surface_parity.rs` | `six_role_names_and_holder_qualified_releases_match_cli_and_mcp` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation_reply_cli.rs` | `accepted_pending_project_create_is_exit_zero_then_local_and_keyed_replay` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description_cli.rs` | `project_descriptions_keep_exact_text_through_create_describe_show_and_retry` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/thread_subscription_cli.rs` | `subscribe_join_wait_and_cancel_use_the_real_api_and_sqlite_paths` | starts the old central (board-central-test-support) for its setup or calls |

### board-central-test-support (3)

| File | Test | Reason |
|---|---|---|
| `src/fixture_shutdown.rs` | `every_cleanup_runs_and_retains_all_sibling_errors` | starts the old central (board-central-test-support) for its setup or calls |
| `src/fixture_shutdown.rs` | `failed_central_start_joins_the_real_jwks_listener` | starts the old central (board-central-test-support) for its setup or calls |
| `src/mail_wire_observer_test.rs` | `push_wire_observation_keeps_item_frame_and_capacity_refusal` | tests the removed old-central fixture crate itself |

### codex-router-host (63)

| File | Test | Reason |
|---|---|---|
| `src/collaboration_runtime/board_composition/received_grants_test.rs` | `initial_create_grant_notice_preserves_pending_registration_and_first_record` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/reenrollment_project_test.rs` | `reenrollment_with_an_existing_writer_renews_and_writes_against_unchanged_central` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/reenrollment_project_test.rs` | `wiped_central_leaves_only_the_unknown_project_read_only_while_new_creation_works` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/snapshot_installation_enrollment_test.rs` | `inline_install_renewal_survives_revocation_readmission_and_a_fresh_session` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/archive_observation.rs` | `archiving_a_held_project_records_its_own_fact_then_finalizes_the_board` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/archive_owner_replacement.rs` | `delayed_archive_observation_cannot_borrow_a_replacement_files_copied_progress` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/central_directory_expiry.rs` | `native_directory_expiry_refresh_keeps_the_host_read_generation` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_cli.rs` | `claim_cli_installs_the_imported_prefix_and_reports_native_not_found` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/claim_future_schema.rs` | `claim_schema_too_new_preserves_the_actual_copy_identity_and_supported_version` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/claim_lost_grant.rs` | `claim_lost_granted_frame_replays_the_same_public_attempt_and_exact_grant` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/claim_notice_overlap.rs` | `claim_and_received_notice_join_one_record_and_publication_while_seed_is_held` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_refusals.rs` | `claim_already_held_preserves_the_native_peer_without_another_seed` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_refusals.rs` | `claim_not_found_and_archived_refusals_create_no_local_writer` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_refusals.rs` | `claim_observes_real_retirement_in_progress_then_retired_without_seeding` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_renewal_order.rs` | `claim_drains_held_runner_renewal_then_persists_its_own_answer_before_publication` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_restart.rs` | `claim_restart_resumes_the_same_install_after_sigkill_without_publishing_seeding` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_restart.rs` | `receiving_graceful_stop_during_install_exits_cleanly_then_resumes_and_posts` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_restart.rs` | `receiving_restart_resumes_acknowledged_install_from_welcome_without_claim` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/claim_restart.rs` | `requested_stop_preserves_a_failed_owned_operation` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/duplicate_machine_identity.rs` | `duplicate_connection_reports_connection_replaced_before_physical_close` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/duplicate_machine_identity.rs` | `duplicate_peer_with_new_session_reports_session_ended_before_physical_close` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/forwarded_post_notifications.rs` | `non_holder_sender_publishes_local_notifications_before_copy_ack_and_deduplicates_retry` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/forwarded_post_sender.rs` | `non_holder_sender_admits_both_post_families_and_replays_at_the_holder` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/forwarded_post_sender.rs` | `non_holder_sender_reports_an_imported_unheld_project` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/forwarded_post_sender.rs` | `non_holder_sender_reports_offline_holder_presence_without_queuing` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/forwarded_post_unknown.rs` | `non_holder_sender_unknown_answer_retries_the_original_post_identity` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/install_attempt_completion_test.rs` | `completed_real_snapshot_install_removes_its_attempt_before_claim_returns` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/messaging_cancel_cases.rs` | `lost_custody_receipt_then_takeover_replays_the_batch_and_cancel_without_new_effects` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_cancel_cases.rs` | `replicated_subscription_end_publishes_the_same_frozen_cancel_after_takeover` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_cancel_cases.rs` | `unreplicated_subscription_end_never_sends_its_cancel_after_takeover` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_cancel_fault_cases.rs` | `lost_receipt_hold_targets_submit_while_sender_query_completes` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_cancel_fault_cases.rs` | `lost_receipt_reply_stays_bound_when_a_cancel_submit_finishes_first` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_inherited_cancel.rs` | `inherited_handoff_cancel_reaches_the_remote_hold_before_any_local_submit_task` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_local_cancel_case.rs` | `held_local_follow_keeps_prevention_when_takeover_discards_its_unreplicated_end` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/messaging_rule_cases.rs` | `takeover_after_replicated_handoff_publishes_one_original_item` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_rule_cases.rs` | `takeover_with_only_replicated_cause_freezes_one_successor_item` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_rule_cases.rs` | `takeover_without_replicated_cause_or_handoff_drops_the_lost_tail` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/messaging_settlement_test.rs` | `project_settle_uses_the_retained_local_project_owner_after_resolution_fails` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/non_holder_task_writes.rs` | `non_holder_task_write_keeps_the_completed_read_copy_epoch` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/non_holder_task_writes.rs` | `non_holder_task_write_uses_fleet_membership_without_a_local_copy` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/notice_acknowledgement.rs` | `received_grant_record_failure_defers_later_notices_then_reconnect_recovers_the_prefix` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/project_claim.rs` | `imported_claim_seeds_verifies_renews_then_publishes_before_first_board_write` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/project_integrity_refusal.rs` | `piece_integrity_refusal_joins_its_sender_without_resend_while_another_project_replicates` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/received_grants.rs` | `received_higher_epoch_replaces_a_held_seed_with_a_new_attempt` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/received_grants.rs` | `received_idle_holder_move_preserves_claimed_prefix_without_an_intervening_write` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/received_grants.rs` | `received_move_then_takeover_round_trip_seeds_each_new_epoch_from_copy` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/received_grants.rs` | `same_epoch_unavailable_writer_is_not_stopped_as_an_older_snapshot_owner` | enrolled machine session against the old central (board composition fixtures) |
| `src/collaboration_runtime/board_composition/tests/returning_holder_expiry.rs` | `membership_expiry_during_a_returning_install_does_not_quarantine_the_project` | starts the old central (board-central-test-support) for its setup or calls |
| `src/collaboration_runtime/board_composition/tests/snapshot_restart.rs` | `snapshot_baseline_is_repaired_before_offline_admission_without_waiting_for_welcome` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `src/collaboration_runtime/startup_contract_tests/duplicate_host_test.rs` | `two_hosts_with_the_same_session_keep_the_latest_connected_without_flip_flop` | enrolled Host: needs the old central's machine session |
| `src/collaboration_runtime/startup_contract_tests/follow_attachment_tests.rs` | `host_follow_attachment_allocates_a_notice_and_retains_only_weak_owners` | enrolled Host: needs the old central's machine session |
| `src/collaboration_runtime/startup_mail_retention_test.rs` | `host_mail_clock_steps_remove_both_holders_at_day_60_and_refuse_day_61` | enrolled Host: needs the old central's machine session |
| `src/enrollment_session_transport_test.rs` | `pending_reenrollment_blocks_the_next_real_socket_until_restart_finishes_reset` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/board_failure_isolation.rs` | `pending_corrupt_feed_is_contained_during_subscription_restart` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_failure_isolation.rs` | `same_readers_corrupt_membership_is_contained_during_subscription_restart` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/claude_peer_subscription_resume.rs` | `claude_peer_subscription_notice_is_refused_while_the_peer_is_closed_and_a_later_post_is_delivered` | enrolled machine session against the old central (board composition fixtures) |
| `tests/integration/production_enrollment_duplicate_test.rs` | `newer_live_session_is_retained_as_refused_by_host_and_installation_status` | enrolled machine session against the old central (board composition fixtures) |
| `tests/integration/production_enrollment.rs` | `failed_session_preparation_keeps_its_typed_failure_on_both_status_surfaces` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/production_enrollment.rs` | `production_host_discovers_enrollment_and_creates_a_central_project` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/production_enrollment.rs` | `production_host_without_enrollment_keeps_ready_and_creation_unreachable` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/remote_wake.rs` | `remote_wake_is_owned_and_fires_on_the_recipient_installation` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/runtime_shutdown_stalled_provider_load.rs` | `shutdown_completes_while_follow_delivery_waits_on_unanswered_provider_load` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/session_message_route_composition.rs` | `host_router_selects_peer_and_provider_without_cross_loading` | starts the old central (board-central-test-support) for its setup or calls |

### collaboration-mcp (4)

| File | Test | Reason |
|---|---|---|
| `src/collaboration_api_router_tests.rs` | `schema_required_fields_reach_each_tool_without_missing_field_errors` | needs a real board through the old central (board_application); carried into PR 4's proof plan |
| `src/collaboration_api_router_tests.rs` | `the_same_call_returns_the_same_structured_result_on_tcp_and_unix` | needs a real board through the old central (board_application); carried into PR 4's proof plan |
| `src/collaboration_api_router_tests/call_lifecycle.rs` | `a_client_that_disconnects_from_a_mutation_waiting_on_the_store_leaves_no_mutation` | needs a real board through the old central (board_application); carried into PR 4's proof plan |
| `src/thread_subscription_entry_tests.rs` | `subscription_tools_roundtrip_through_mcp_api_native_projects_and_local_progress` | needs a real board through the old central (board_application); carried into PR 4's proof plan |

### collaboration-service (495)

| File | Test | Reason |
|---|---|---|
| `src/project_admission/behavior_tests/activation_boundaries.rs` | `activation_is_the_request_position_and_reactivation_changes_the_local_key` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activation_boundaries.rs` | `policy_change_preserves_backlog_releases_old_selection_and_fences_late_settlement` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `project_main_tracking_rejects_negative_and_future_boundaries` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `project_watch_boundary_rejects_negative_and_future_stored_values` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `scoped_bookmarks_reject_negative_and_future_boundaries` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `sqlite_rejects_non_boolean_watch_and_summary_interest_values` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `watch_project_accepts_an_empty_activity_stream` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/activity_boundaries.rs` | `watch_status_rejects_negative_and_future_start_boundaries` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/archived_membership.rs` | `archived_reader_opt_out_and_keyed_marks_pass_while_work_and_new_interest_are_refused` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/covered_watches.rs` | `p6_join_without_watch_keeps_its_disabled_row_on_later_posts` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/covered_watches.rs` | `p6_project_coverage_does_not_reactivate_an_explicitly_unwatched_thread` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/covered_watches.rs` | `q2_project_subscribe_reactivates_a_plain_opt_out_without_a_thread_subscription_override` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/covered_watches.rs` | `subscription_activation_creates_existing_watches_and_post_replay_keeps_their_boundary` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/flight_regressions.rs` | `p4_policy_patch_during_flight_releases_the_matching_window` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/flight_regressions.rs` | `p5_resolve_during_flight_release_clears_selection_and_allows_selection` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `a_claim_refuses_an_enrollment_pair_for_a_different_service_under_the_lock` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `a_ledger_write_failure_rolls_back_the_frozen_handoff_and_intent_transition_together` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `a_missing_enrollment_defers_without_writes_and_a_resolved_pair_can_retry` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `an_unknown_installation_is_a_terminal_refusal_without_a_frozen_handoff` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `intent_claim_commits_the_frozen_item_and_equal_reference_and_replays_without_reallocation` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/intent_handoff.rs` | `the_admission_writer_refuses_a_handed_off_reference_that_names_another_frozen_item` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/project_admission/behavior_tests/membership_completion.rs` | `leave_replacement_and_expiry_end_rows_and_delete_windows` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_completion.rs` | `leave_resolve_ends_resolver_as_left_and_drains_other_readers` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_completion.rs` | `resolve_drains_pending_or_ends_mode_off_subscription` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_corruption.rs` | `corrupted_subscription_enum_fails_closed_with_field_name` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_corruption.rs` | `expiration_does_not_normalize_a_corrupt_subscription_row` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_corruption.rs` | `m2_corrupt_ended_row_reports_the_missing_ended_at_field` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_drain.rs` | `m3_unsubscribe_after_expiry_keeps_the_expired_end_reason` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_drain.rs` | `m6_draining_thread_subscription_completes_as_resolved_after_settlement` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_drain.rs` | `p7_drop_policy_processes_a_draining_subscription` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_lifecycle.rs` | `create_with_role_subscribes_the_session_creator` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_lifecycle.rs` | `join_watch_creates_default_subscription_and_no_watch_does_not` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_lifecycle.rs` | `rejoin_reactivates_ended_subscription_and_keeps_policy` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_lifecycle.rs` | `thread_subscribe_checks_participant_validates_patch_and_activates_watch` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_lifecycle.rs` | `unsubscribe_keeps_watch_but_unwatch_ends_without_post_reactivation` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_renewal.rs` | `explicit_renewal_extends_policy_lifetime_and_off_rows_still_expire` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/membership_renewal.rs` | `only_reader_posts_renew_the_subscription` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_corruption.rs` | `participant_cursor_is_thread_bound_and_unknown_stored_role_is_rejected` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_corruption.rs` | `thread_list_rejects_corrupt_projected_orchestrator_records` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_delivery.rs` | `subscription_handoff_keeps_last_seen_monotonic_and_participant_changes_stay_out_of_inbox` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_gates.rs` | `concurrent_orchestrator_joins_commit_one_holder_and_report_the_winner` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_gates.rs` | `session_thread_subscriptions_require_join_while_project_humans_remain_exempt` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_gates.rs` | `thread_subscriptions_refuse_each_missing_session_participant_without_creating_watches` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_intervals.rs` | `create_and_repeated_joins_retain_history_and_one_open_participant` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_intervals.rs` | `human_create_without_role_has_no_participant_and_session_create_changes_only_rejection_ledger` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_intervals.rs` | `implementer_seat_is_unique_replaceable_and_visible_on_thread_reads` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_lifecycle.rs` | `archived_leave_resolve_refuses_without_changing_participant_or_watch_state` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_lifecycle.rs` | `leave_resolve_publishes_unread_to_another_active_watcher` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/participant_lifecycle.rs` | `replace_handover_and_human_resolve_are_atomic_position_transitions` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/post_cooldown.rs` | `competing_top_level_posts_for_one_actor_commit_only_once` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/post_cooldown.rs` | `cooldown_allows_a_top_level_post_at_the_sixty_second_boundary` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/post_cooldown.rs` | `malformed_cooldown_post_timestamps_are_rejected_as_attributed_invalid_records` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/post_cooldown.rs` | `thirty_seconds_is_still_inside_the_project_cooldown` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_coverage.rs` | `project_subscription_keeps_off_activity_unread_and_thread_rows_take_precedence` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_notice_boundaries.rs` | `empty_project_subscription_covers_its_first_later_root_with_a_pending_window` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_notice_boundaries.rs` | `p2_project_root_notice_settlement_keeps_inbox_fetch_and_ack_valid` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_watch_fidelity.rs` | `only_active_unexpired_project_subscribers_materialize_new_thread_watches` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_watch_fidelity.rs` | `plain_project_watch_never_covers_replies_state_changes_or_thread_reads` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/project_watch_fidelity.rs` | `project_subscription_reactivates_disabled_rows_except_a_retained_thread_override` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/reactivation_regressions.rs` | `old_settlement_after_cancel_and_resubscribe_preserves_replacement_window_and_outcome` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/reactivation_regressions.rs` | `p3_late_settlement_after_rejoin_cannot_cross_the_new_watch_boundary` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/reactivation_regressions.rs` | `p8_first_join_by_existing_watcher_starts_after_prior_activity` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/reactivation_regressions.rs` | `p8_rejoin_after_unsubscribe_starts_after_activity_while_unsubscribed` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/reactivation_regressions.rs` | `q1_rejoin_while_active_keeps_pending_window` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/read_agreement.rs` | `rewatch_boundaries_and_acknowledgements_keep_summary_equal_to_inbox` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_notices.rs` | `subscription_notice_byte_budget_selects_a_prefix_before_in_flight_commit` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_notices.rs` | `subscription_notice_caps_roots_without_settling_omitted_due_windows` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_notices.rs` | `subscription_notice_returns_only_ranges_and_counts_and_keeps_posts_unread` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_recovery.rs` | `restart_before_notice_settlement_reselects_the_same_range` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_recovery.rs` | `restore_clears_interrupted_selection_marks_and_keeps_draining_window_deadline` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_release.rs` | `release_subscription_preserves_unallocated_window_across_arrivals` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_release.rs` | `release_subscription_restores_due_without_advancing_delivered` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_release.rs` | `release_subscription_stale_window_or_through_fence_is_a_noop` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_selection_fences.rs` | `receipts_require_exact_window_generation_and_frozen_through_before_and_after_arrivals` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement_storage.rs` | `m7_rescan_prunes_a_window_with_no_pending_posts` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement_storage.rs` | `rescan_reopens_a_missing_window_from_pending_activity` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement_storage.rs` | `settlement_failure_rolls_back_delivered_window_and_outcome_together` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement.rs` | `arrivals_after_release_do_not_restart_the_original_window_cap` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement.rs` | `drop_advances_delivered_but_keeps_activity_unread` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement.rs` | `drop_storage_primitive_does_not_skip_hold_policy_activity` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_settlement.rs` | `late_lower_settlement_never_moves_delivered_backwards` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_timing.rs` | `post_windows_exclude_author_and_project_roots_keep_independent_timing` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_timing.rs` | `residual_arrival_opens_at_first_arrival_while_batch_is_in_flight` | SubscriptionFixture registers its project through the old central's session |
| `src/project_admission/behavior_tests/window_timing.rs` | `residual_window_that_reaches_its_cap_is_due_immediately_after_settlement` | SubscriptionFixture registers its project through the old central's session |
| `src/subscription_delivery/intent_progress_tests.rs` | `a_recorded_push_refusal_is_settled_after_a_transient_lease_failure` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_progress_tests.rs` | `deferred_history_does_not_grow_the_next_intent_pass` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_progress_tests.rs` | `deleting_the_derived_cursor_preserves_exactly_once_consumption` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_progress_tests.rs` | `failed_push_preparation_holds_the_contiguous_prefix_and_retries_the_claim` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_progress_tests.rs` | `restart_skips_consumed_history_even_when_its_body_is_corrupt` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_reinstatement_recovery_tests.rs` | `restart_after_reinstatement_commit_does_not_lose_an_already_consumed_identity` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_role_delivery_test.rs` | `claimed_retained_holder_delivers_an_already_consumed_reinstatement_in_the_same_pass` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_role_delivery_test.rs` | `deferred_human_claim_delivers_a_new_local_coholder_in_the_same_consumer_pass` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/intent_role_delivery_test.rs` | `handed_off_role_intent_delivers_to_the_frozen_recipient_after_holders_change` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests.rs` | `ordinary_project_submission_records_the_shared_clock_time` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests.rs` | `running_reader_stores_one_neutral_push_before_layer_zero_and_keeps_inbox_unread` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/activity_scope_proofs.rs` | `lagged_activity_rescans_the_reader_and_recovers_a_different_project` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/control_view_proofs.rs` | `reconcile_receipt_reports_the_failed_pass_instead_of_waiting_for_a_later_scan` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/control_view_proofs.rs` | `reconcile_receipt_waits_for_the_owner_to_finish_its_native_window_scan` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/control_view_proofs.rs` | `subscriptions_list_reconciles_a_subscriber_whose_scan_has_not_run` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `a_failed_hold_write_leaves_the_dm_unmarked_and_the_backoff_holds_it` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `a_failed_position_capture_still_delivers_a_dm_to_a_running_target` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `attempted_dm_on_restart_becomes_unknown_and_never_reenters_layer_zero` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `board_notification_wakeable_hold_recovers_one_exact_push_when_running` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `dm_settlement_failure_retries_the_write_without_duplicate_delivery` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `live_elsewhere_presence_reaches_authoritative_router_rejection_and_is_never_held_or_replayed` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `new_auto_to_unloaded_target_attempts_once_with_permission_to_load` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `queue_and_previously_held_auto_wait_until_loaded_and_recover_their_exact_mode` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `restart_gap_preserves_steer_guard_and_race_rejection_never_holds` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `restarted_nonrunning_steer_and_guarded_pending_records_are_rejected` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `restored_guarded_generation_mismatch_is_terminal_without_any_broadened_attempt` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `same_target_dm_and_subscription_have_one_inflight_delivery` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `steer_and_guarded_send_to_nonrunning_reject_without_any_hold` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/direct_message_proofs.rs` | `unavailable_board_holds_dm_until_a_load_signal_with_no_presence_timer` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/f6_held_record_proofs.rs` | `cleanup_write_retry_does_not_block_reader_reconcile` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/f6_held_record_proofs.rs` | `ending_one_scope_preserves_a_held_batch_with_another_active_root` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/failed_pass_backoff_proofs.rs` | `a_failed_held_push_cleanup_backs_off_from_one_to_thirty_seconds_and_logs_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/failed_pass_backoff_proofs.rs` | `a_failed_owner_pass_backs_off_from_one_to_thirty_seconds_and_logs_once_per_run` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/failed_pass_backoff_proofs.rs` | `deferred_startup_recovery_backs_off_from_one_to_thirty_seconds_and_logs_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `arrival_during_flight_gets_its_own_quiet_window_and_elapsed_cap` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `cancel_during_flight_settles_the_passed_range_and_prevents_later_pushes` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `mode_change_during_flight_settles_original_push_and_polls_released_backlog` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `one_owner_serializes_push_and_wait_and_does_not_hold_the_board_lock` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `restart_after_cap_delivers_immediately_with_downtime_arrivals_included` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/flight_proofs.rs` | `restart_mid_quiet_preserves_the_original_arrival_deadline` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/follow_admission_proofs.rs` | `follow_admission_does_not_allocate_a_missing_membership` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/follow_admission_proofs.rs` | `follow_admission_rechecks_generation_and_activation_before_allocating` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/follow_admission_proofs.rs` | `follow_admission_refuses_overlapping_ranges_within_and_across_handoffs` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/follow_admission_proofs.rs` | `poll_project_membership_freezes_one_handoff_per_root_and_rechecks_shape` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `drop_then_push_leaves_both_messages_unread` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `expiry_without_activity_sends_one_stored_renew_command` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `hold_keeps_each_arrival_in_a_frozen_item_and_delivers_it_once_on_running` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `unknown_outcome_advances_delivered_and_preserves_evidence` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `unreachable_wake_is_held_with_visible_reason` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/lifecycle_proofs.rs` | `wake_uses_may_load_only_for_a_wakeable_target` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/local_mail_proofs.rs` | `aged_held_mail_survives_store_reopen_and_delivers_full_content_through_the_same_owner` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/local_mail_proofs.rs` | `concurrent_same_item_reports_pending_then_replays_the_exact_settled_outcome` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/local_mail_proofs.rs` | `disconnected_sender_waiter_keeps_custody_and_only_an_omitted_id_creates_new_work` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/local_mail_proofs.rs` | `local_native_attempt_is_drained_when_runtime_shutdown_starts` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/membership_expiry_admission_proofs.rs` | `concurrent_renewal_and_expiry_commit_one_of_the_two_serialized_states` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/membership_expiry_admission_proofs.rs` | `expiry_admission_rechecks_a_membership_renewed_after_the_proposal_was_read` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/membership_expiry_admission_proofs.rs` | `expiry_admission_writes_one_router_end_and_is_idempotent` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_activation_frontier.rs` | `cancelled_activation_settles_after_reactivation_and_acknowledged_reopen` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_activation_frontier.rs` | `resolved_activation_stays_owed_after_reactivation_until_its_frozen_replacement_delivers` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_cancel_backpressure_proofs.rs` | `a_full_native_outbox_keeps_its_frozen_child_pending_until_real_capacity_returns` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_cancel_proofs.rs` | `a_real_membership_end_prevents_its_held_follow_through_a_frozen_child` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_cancel_proofs.rs` | `a_resolved_membership_keeps_its_held_item_and_drains_the_unallocated_range` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_expiry_proofs.rs` | `a_renewed_held_follow_delivers_past_its_original_membership_expiry` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_expiry_proofs.rs` | `crossing_expiry_during_the_membership_read_keeps_the_reader_deadline` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_expiry_proofs.rs` | `startup_expires_an_allocated_follow_even_without_local_custody_or_live_public_rows` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_expiry_proofs.rs` | `the_existing_reader_expiry_cadence_commits_one_router_end_and_cancel_group` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_grouping_proofs.rs` | `one_membership_drains_twenty_roots_then_the_remaining_root_without_lost_ranges` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_grouping_proofs.rs` | `resumed_reader_keeps_separate_membership_items_until_a_load_signal` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_injection_proofs.rs` | `a_follow_recipient_with_no_sender_project_file_can_still_inject_normally` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_injection_proofs.rs` | `a_native_forced_tail_keeps_prevention_after_the_group_is_discarded_from_the_successor_file` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_injection_proofs.rs` | `a_written_unacknowledged_group_prevents_local_injection_without_calling_the_native_client` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_mail_proofs.rs` | `held_follow_keeps_its_frozen_item_ranges_and_link_until_one_native_acceptance` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_outcome_fault_proofs.rs` | `a_failed_follow_projection_keeps_one_frozen_item_for_the_next_project_hint` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_follow_outcome_fault_proofs.rs` | `a_follow_outcome_write_fault_recovers_without_a_second_native_call` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_frontier_proofs.rs` | `a_terminal_follow_stays_above_frontier_until_its_generation_child_is_frozen_and_settled` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_frontier_proofs.rs` | `quiet_local_delivery_releases_acknowledged_frontier_without_erasing_retained_custody` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_frontier_proofs.rs` | `two_prevented_generations_keep_a_resolved_drain_above_frontier_until_final_delivery` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_generation_isolation_proofs.rs` | `an_expired_lease_queries_an_eligible_remote_cancel_and_renewal_replays_it_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_generation_isolation_proofs.rs` | `local_generation_replacement_reaches_native_delivery_while_another_readers_query_is_held` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_generation_prevention_proofs.rs` | `a_generation_prevented_range_is_replaced_once_and_reaches_the_native_client` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_generation_prevention_proofs.rs` | `forged_prevention_references_are_refused_before_the_locked_replacement_allocation` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_handoff_recovery_proofs.rs` | `committed_handoff_recovers_the_same_item_after_reopen_before_outbox_admission` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `a_failed_position_capture_fails_the_pass_and_the_retry_keeps_the_mail_held` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `a_failed_position_capture_still_releases_mail_to_a_running_target` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `a_failed_presence_check_marks_nothing_and_the_backoff_checks_again` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `a_failed_release_unmarks_its_mail_and_the_backoff_releases_it` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `a_later_check_keeps_the_held_mails_earlier_position_so_a_load_between_them_owes_a_check` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_fault_proofs.rs` | `an_expired_journal_position_owes_one_check_which_records_a_fresh_position` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `a_codex_thread_that_loads_during_the_check_still_releases_its_held_mail` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `a_journal_record_for_another_thread_checks_nothing_and_its_own_load_releases` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `explicit_reader_reconcile_releases_native_held_mail_before_the_next_presence_tick` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `held_mail_whose_target_is_not_running_goes_back_through_delivery_instead_of_staying_held` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `held_native_mail_waits_for_a_load_signal_and_holds_no_presence_timer` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_held_mail_proofs.rs` | `mail_held_when_the_runtime_reopens_for_a_target_not_running_is_refused_on_the_first_check` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_intent_publisher_proofs.rs` | `a_frozen_local_intent_uses_native_custody_and_replay_does_not_recreate_its_pruned_push` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_intent_publisher_proofs.rs` | `a_held_local_intent_keeps_its_frozen_item_and_link_until_the_reader_runs` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_owed_transfer_proofs.rs` | `completed_move_seed_delivers_the_inherited_owed_range_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_owed_transfer_proofs.rs` | `completed_takeover_seed_delivers_only_the_confirmed_owed_range_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_gate_proofs.rs` | `a_temporary_poll_check_failure_reconsiders_the_same_item_on_an_existing_rescan` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_gate_proofs.rs` | `poll_without_its_publisher_stays_held_until_the_same_wait_is_reconciled` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_incoming_proofs.rs` | `remote_incoming_poll_uses_its_frozen_policy_without_opening_the_senders_project` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_mail_proofs.rs` | `a_poll_sdk_item_waits_without_a_push_then_hands_over_its_own_identity_once` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_mail_proofs.rs` | `three_poll_roots_keep_filtered_and_fifo_custody_independent` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_recovery_proofs.rs` | `a_disconnected_poll_waiter_keeps_the_item_for_the_next_wait_without_unknown` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_recovery_proofs.rs` | `held_poll_policy_and_original_identity_survive_physical_owner_reopen` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_poll_recovery_proofs.rs` | `interruption_after_b_handover_leaves_b_uncertain_and_c_deliverable_after_reopen` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_readiness_interleaving_proofs.rs` | `a_second_readiness_reconcile_while_sdk_attempt_is_pending_preserves_backoff` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_successor_frontier_proofs.rs` | `a_live_frontier_releases_new_items_without_sender_retire_and_reheld_owner_keeps_progress` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_successor_frontier_proofs.rs` | `a_successor_starts_frontier_cleanup_at_the_seeded_floor_without_replaying_old_custody` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_successor_frontier_proofs.rs` | `an_inherited_reader_scans_new_epoch_activity_from_its_valid_activation_cursor` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_window_lineage_proofs.rs` | `a_mixed_epoch_window_freezes_round_trips_and_passes_actual_admission` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_window_lineage_proofs.rs` | `discarded_window_and_selection_resume_without_losing_or_duplicating_current_activity` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/native_window_lineage_proofs.rs` | `unknown_epoch_and_future_position_cursors_are_refused_without_rewriting_progress` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/owner_activity_proofs.rs` | `activity_waiter_does_not_complete_at_a_barrier_before_the_post_is_selected` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/push_store_proofs.rs` | `expiry_after_resubscribe_stores_a_reference_for_the_new_generation` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/push_store_proofs.rs` | `failed_poll_attempt_mark_keeps_the_frozen_item_retryable_and_returns_explicit_error` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/push_store_proofs.rs` | `successive_selected_activity_batches_store_distinct_canonical_origin_refs` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/queued_proofs.rs` | `acp_queue_known_not_submitted_returns_to_hold_without_advancing` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/queued_proofs.rs` | `acp_queue_still_unknown_settles_unknown_with_route_evidence` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/queued_proofs.rs` | `declared_renewal_before_covering_wait_changes_only_the_covered_membership` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/queued_proofs.rs` | `loaded_only_acp_queue_is_reconciled_before_any_settlement` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/restore_corruption_proofs.rs` | `corrupt_recovery_target_row_does_not_strand_held_dm_after_restart` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/restore_corruption_proofs.rs` | `corrupt_subscription_mode_is_skipped_while_valid_reader_restores_and_delivers` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/restore_corruption_proofs.rs` | `failed_direct_message_recovery_query_does_not_stop_valid_subscription_reader` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/restore_corruption_proofs.rs` | `shared_progress_store_failure_is_not_contained_as_a_project_failure` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/stall_diagnosis.rs` | `a_failed_scan_returns_typed_unavailable_at_the_pending_waiters_deadline` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/stall_diagnosis.rs` | `runtime_timer_clock_reaches_due_selection_and_prepared_delivery` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/stall_diagnosis.rs` | `virtual_timeout_can_expire_while_the_due_query_waits_for_a_native_project_reader` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_recovery_regressions.rs` | `deferred_handoff_replaces_an_owner_that_accepts_then_drops_its_command` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_recovery_regressions.rs` | `deferred_project_restores_an_abandoned_selection_despite_the_same_readers_healthy_owner` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_recovery_regressions.rs` | `recovering_project_does_not_wait_for_an_owner_without_membership` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_recovery_regressions.rs` | `terminal_project_removal_survives_a_later_shared_failure_across_retry_passes` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `deferred_discovery_preserves_a_selection_owned_by_a_reader_started_after_startup` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `definitive_failure_after_a_deferred_probe_quarantines_and_ends_recovery` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `early_startup_timeout_recovers_owed_range_without_reader_action` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `healthy_project_delivers_while_another_startup_probe_is_deferred` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `shutdown_drains_a_pending_startup_discovery_retry` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/startup_retry_proofs.rs` | `startup_auto_advance_does_not_expire_the_healthy_projects_owed_notice` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/subscription_observation_proofs.rs` | `a_resolved_membership_stays_visible_while_its_allocated_item_is_held` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/subscription_observation_proofs.rs` | `failed_delivery_retries_at_its_scheduled_time_then_clears_the_public_fields` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/subscription_observation_proofs.rs` | `latest_observed_outcome_wins_over_newer_allocation_and_counts_both_held_items` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/subscription_observation_proofs.rs` | `native_retry_observations_reach_the_public_view_and_clear_after_readiness` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/timing_proofs.rs` | `activity_in_one_root_never_delays_another_root` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/timing_proofs.rs` | `quiet_period_restarts_from_the_second_arrival` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/timing_proofs.rs` | `rejected_delivery_backoff_doubles_from_thirty_seconds_to_ten_minutes` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/timing_proofs.rs` | `repeated_ninety_second_arrivals_are_passed_at_the_fifteen_minute_cap` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/transient_scan_proofs.rs` | `maintenance_timeout_retries_without_quarantining_the_project` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/wait_proofs.rs` | `concurrent_session_waits_are_fifo_and_never_receive_the_same_range` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/wait_proofs.rs` | `dropping_the_last_service_handle_cancels_its_owned_tasks` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/wait_proofs.rs` | `finite_wait_budget_hands_oldest_root_then_preserves_remaining_due_root` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/wait_proofs.rs` | `human_poll_returns_bodyless_ranges_without_storing_or_pushing` | OwnerFixture registers its workspace, board and project through the old central's session |
| `src/subscription_delivery/tests/wait_proofs.rs` | `poll_wait_timeout_is_empty_and_non_poll_wait_is_rejected` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/accepted_prefix_admission.rs` | `a_bootstrapped_file_reopened_as_provisional_is_superseded` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/accepted_prefix_admission.rs` | `a_current_epoch_acknowledgement_covers_an_inherited_stand_down_for_the_exact_reader` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/accepted_prefix_admission.rs` | `a_filename_epoch_that_differs_from_the_native_file_is_refused` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/accepted_prefix_admission.rs` | `cold_metadata_from_another_project_is_refused_without_rewriting_the_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/accepted_prefix_admission.rs` | `inherited_cursors_keep_marks_and_late_anchors_while_new_close_coordinates_use_the_admitted_epoch` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/acknowledgement_scope_identity.rs` | `duplicate_thread_spellings_reject_before_marks_or_either_ledger_key` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/admission_gate_mcp.rs` | `a_future_schema_refuses_mcp_writes_without_a_ledger_or_feed_append` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/admission_gate_mcp.rs` | `a_seeded_writer_replays_known_requests_and_evaluates_unknown_requests` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/admission_gate_mcp.rs` | `native_expiry_does_not_record_a_request_and_same_epoch_renewal_evaluates_it_again` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/admission_gate_mcp.rs` | `schema_mismatch_is_a_gate_result_and_expiry_still_has_priority` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_cursor_order.rs` | `aggregate_continuation_keeps_every_post_when_router_wall_clock_moves_backwards` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/aggregate_position_recovery.rs` | `a_later_exhausted_range_page_retains_its_signed_retry` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_position_recovery.rs` | `after_position_retains_unavailable_b_behind_the_sweep` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_position_recovery.rs` | `an_initially_unavailable_only_range_retains_its_signed_retry` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_position_recovery.rs` | `an_unavailable_only_page_terminates_without_losing_saved_positions` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_position_recovery.rs` | `inclusive_range_retains_the_unavailable_boundary_record` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_scope_sweep.rs` | `post_list_continuation_admits_a_project_created_mid_browse_beyond_the_sweep` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_scope_sweep.rs` | `unread_continuation_admits_a_new_watch_only_beyond_the_merge_sweep` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_sweep_regressions.rs` | `a_multi_record_page_keeps_its_extreme_rather_than_its_last_key` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_sweep_regressions.rs` | `ascending_sweep_keeps_the_maximum_across_clock_regression` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_sweep_regressions.rs` | `descending_sweep_keeps_the_minimum_across_clock_regression` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_sweep_regressions.rs` | `equal_time_sweep_uses_project_and_position_and_keeps_nanosecond_order` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/aggregate_unavailable_paging.rs` | `exhausted_healthy_pages_end_while_reporting_a_too_new_project` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/archived_read_marks.rs` | `an_archived_content_read_advances_its_mark_without_activity_or_a_request_key` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/archived_read_marks.rs` | `an_archived_read_answers_data_but_cannot_bypass_unacknowledged_stand_down` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/authority_and_fact_corrections.rs` | `every_lifecycle_fact_is_operation_owned_and_rejected_from_producers` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/authority_and_fact_corrections.rs` | `most_recent_released_lead_can_return_only_by_self_grant_without_owner_grant` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/authority_and_fact_corrections.rs` | `producer_facts_need_at_least_one_subject_link` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/authority_and_fact_corrections.rs` | `unauthorized_same_holder_grants_reject_and_replay_before_authorized_noops` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_admission_mcp.rs` | `native_mcp_admission_replays_decisions_and_isolates_project_authority` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/board_behaviors.rs` | `immutable_posts_threads_references_and_signed_pagination_share_one_project_order` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_behaviors.rs` | `metadata_and_repository_operations_preserve_identity_and_lifecycle` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_concurrency_behaviors.rs` | `archive_and_post_race_observes_both_valid_serial_orders` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_concurrency_behaviors.rs` | `concurrent_inbox_reads_preserve_summaries_and_report_latency` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_concurrency_behaviors.rs` | `resolve_and_thread_post_race_observes_both_valid_serial_orders` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_control_path.rs` | `board_control_roundtrip_preserves_root_thread_and_actor` | central-backed board fixture |
| `tests/integration/board_control_path.rs` | `control_board_failures_pagination_and_inbox_use_the_public_path` | central-backed board fixture |
| `tests/integration/board_control_path.rs` | `control_participant_operations_preserve_typed_state_and_authorization` | central-backed board fixture |
| `tests/integration/board_control_path.rs` | `control_thread_list_pages_escape_heavy_holders_without_skips_or_repeats` | central-backed board fixture |
| `tests/integration/board_control_path.rs` | `control_uuid_holder_writes_report_scope_growth_latency` | central-backed board fixture |
| `tests/integration/board_control_support/validation.rs` | `malformed_board_requests_return_safe_specific_failures` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_lifecycle_contracts.rs` | `board_update_preserves_history_watch_bookmark_and_thread_list_filter` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_lifecycle_contracts.rs` | `thread_root_failures_stay_specific_and_directory_collisions_create_no_native_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_lifecycle_contracts.rs` | `unresolve_changed_and_already_so_allow_replies_on_an_active_board` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_lookup_failures.rs` | `missing_resource_uses_the_canonical_inspection_action` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_lookup_failures.rs` | `missing_root_uses_inspect_resource_and_summary_reads_cover_many_workspaces` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_metadata_records.rs` | `corrupt_metadata_listing_names_the_resource_and_canonical_inspection_action` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_metadata_records.rs` | `large_metadata_lists_continue_before_the_control_frame_limit` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_metadata_records.rs` | `malformed_stored_primary_id_is_reported_without_echoing_the_raw_value` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_read_error_corrections.rs` | `every_project_entity_read_uses_the_board_error_family` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_read_error_corrections.rs` | `valid_read_actors_do_not_take_the_blame_for_malformed_read_fields` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_replay_cases_mcp.rs` | `closed_work_keeps_late_evidence_and_conversation_answers` | central-backed board fixture |
| `tests/integration/board_replay_cases_mcp.rs` | `closed_work_refuses_unanchored_facts_and_new_owner_rows` | central-backed board fixture |
| `tests/integration/board_replay_cases_mcp.rs` | `lead_replacement_history_answers_before_and_after_succession` | central-backed board fixture |
| `tests/integration/board_replay_cases_mcp.rs` | `messaging_history_answers_the_three_questions` | central-backed board fixture |
| `tests/integration/board_replay_cases_mcp.rs` | `replaced_leads_and_used_grants_keep_their_refusals` | central-backed board fixture |
| `tests/integration/board_repository_ownership.rs` | `foreign_local_repositories_are_refused_on_every_request_that_names_one` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_repository_ownership.rs` | `workspace_repository_filter_matches_typed_locator_fields` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_search_behaviors.rs` | `archive_removes_remaining_search_hits_without_invalidating_the_cursor` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_search_behaviors.rs` | `discovery_pages_bound_full_parent_context_without_losing_projects` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_search_behaviors.rs` | `large_post_search_pages_respect_bytes_and_report_scan_cost` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_search_behaviors.rs` | `searches_keep_text_location_kind_and_reader_state_independent` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/board_write_scale.rs` | `real_size_project_measures_admitted_posts_and_task_updates` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_entity_reads.rs` | `entity_page_cost_does_not_grow_with_unreturned_tasks` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_feed_corruption.rs` | `unread_excludes_only_the_same_session_endpoint` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_feed_corruption.rs` | `unselected_unwatched_corrupt_post_cannot_break_a_workspace_page` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_feed_costs.rs` | `post_and_inbox_pages_have_size_independent_costs` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_fleet_reads.rs` | `fleet_pages_and_discovery_parent_batches_do_not_grow_with_directory_size` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_fleet_writes.rs` | `fleet_write_reads_only_its_operands_and_survives_an_unrelated_corrupt_cache_record` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/bounded_gate_reads.rs` | `covered_reads_and_session_context_have_operand_bounded_cost` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_membership_reads.rs` | `writes_reconcile_only_their_project_and_reader_wide_reads_probe_once` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_participant_pages.rs` | `participant_page_work_stays_bounded_with_same_reader_history` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_pending_cards.rs` | `non_lead_pending_metadata_costs_ignore_foreign_tagged_batches` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_post_lookup.rs` | `post_lookup_costs_ignore_unopened_project_files` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_read_costs.rs` | `one_thread_page_has_size_independent_statement_and_decode_counts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_thread_history.rs` | `thread_show_does_not_decode_unrelated_project_posts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_thread_reads.rs` | `thread_page_never_decodes_another_workspaces_corrupt_thread` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_thread_reads.rs` | `watched_keyset_honors_thread_opt_out_and_validates_the_start_position` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_trail_reads.rs` | `a_backward_decision_page_has_size_independent_cost` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_view_reads.rs` | `card_reads_current_state_and_a_bounded_recent_prefix` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_view_scope.rs` | `an_unassigned_task_card_does_not_read_an_unrelated_task_body` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_view_scope.rs` | `owner_and_lead_load_do_not_load_an_irrelevant_projects_bodies` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_window_reads.rs` | `subscription_and_content_pages_bound_their_body_and_followup_reads` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/bounded_write_costs.rs` | `representative_writes_have_operand_bounded_statement_and_decode_counts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/canonical_project_text.rs` | `native_current_and_kind_reads_reject_padded_names_titles_and_tags` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/canonical_project_text.rs` | `native_immutable_feed_rejects_padded_names_titles_and_tags` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/canonical_project_text.rs` | `participant_reads_reject_noncanonical_notes_and_joining_feed_metadata` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks.rs` | `acknowledgement_mapping_remains_readable_under_the_project_writer_guard` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks.rs` | `published_snapshot_baseline_uses_its_receipt_and_is_unchanged_after_later_writes` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks.rs` | `split_capture_maps_only_acknowledged_commits_and_survives_lost_answers` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks/cancel_expansions.rs` | `child_capture_marks_wait_for_final_ack_and_survive_all_native_owners_reopening` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks/cancel_expansions.rs` | `discarded_child_ordinal_reuse_does_not_transfer_a_delayed_old_epoch_acknowledgement` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/capture_feed_marks/cancel_expansions.rs` | `verified_seed_baseline_inherits_child_marks_and_later_expansions_cannot_change_it` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/card_intent_reopen.rs` | `reopened_cards_keep_project_intents_and_only_the_relevant_subject_intents` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/card_response_budget_mcp.rs` | `escaped_latest_trail_is_a_bounded_prefix_without_losing_the_recorded_count` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/context_scope_corrections.rs` | `context_excludes_projects_outside_the_sessions_repository` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/context_scope_corrections.rs` | `context_reads_roles_and_scope_facts_without_decoding_posts_or_feed` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/context_scope_corrections.rs` | `corrupt_unrelated_project_does_not_poison_a_healthy_session_context` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/context_scope_corrections.rs` | `provider_context_uses_existing_durable_session_worktrees` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/context_scope_corrections.rs` | `unresolved_session_metadata_never_invents_an_allowed_context` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/covered_read_marks_mcp.rs` | `an_expired_project_still_returns_the_read_and_never_allocates_a_mark` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/covered_read_marks_mcp.rs` | `filtered_windows_and_completed_decision_ranges_advance_only_their_covered_positions` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/covered_read_marks_mcp.rs` | `superseded_project_reads_are_unavailable_and_allocate_no_mark` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/covered_read_marks_mcp.rs` | `too_new_schema_is_refused_unchanged_and_mismatched_schema_keeps_readable_data` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/cross_project_behavior.rs` | `cross_project_references_and_unread_summaries_keep_independent_scope` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/filtered_inboxes.rs` | `latest_is_nonmutating_and_continues_normally_when_watches_change` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/filtered_inboxes.rs` | `project_filter_keeps_global_watches_without_subscribing_to_project_threads` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/fleet_client.rs` | `central_reply_cache_and_original_outbox_input_survive_restart` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_client.rs` | `offline_project_creation_waits_for_the_central_decision_before_registration` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_lifecycle_mcp.rs` | `board_close_accepts_only_initiatives_and_keeps_replay_and_noop_meaning` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/fleet_lifecycle_mcp.rs` | `supplied_reports_finish_one_and_multiple_boards_in_the_same_fleet_decision` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/fleet_lifecycle_mcp.rs` | `workspace_archive_cascades_atomically_and_retains_pending_project_reports` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/fleet_metadata_mcp.rs` | `a_thread_can_discuss_an_immutable_adr_in_its_workspace` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_metadata_mcp.rs` | `adr_lifecycle_is_replayable_through_mcp` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_metadata_mcp.rs` | `project_show_reads_the_existing_fleet_directory_record` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_registration_start.rs` | `a_newly_decided_directory_starts_registration_without_another_recovery_pass` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/fleet_registration.rs` | `settled_directory_requests_are_durable_before_registration_and_recovered_after_reopen` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/forwarded_post_origin.rs` | `forwarded_admission_refuses_operations_outside_the_post_family_without_effects` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/forwarded_post_origin.rs` | `forwarded_posts_keep_the_declared_actor_and_original_per_observation_origin_on_replay` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/held_queue_depth.rs` | `records_sequential_admission_costs_at_held_depths_and_the_real_capacity_boundary` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_acknowledgement_mcp.rs` | `batch_acknowledgements_keep_completed_outcomes_after_an_unavailable_scope` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_acknowledgement_mcp.rs` | `routed_refusals_keep_the_ledger_and_unreadable_retry_outcomes_stay_unknown` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_cursor_contracts.rs` | `first_unread_initialization_and_post_assign_activity_to_history_or_unread_atomically` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_cursor_contracts.rs` | `inbox_cursors_bind_scope_mode_reader_and_store_without_initialization_on_error` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_initializations_mcp.rs` | `aggregate_initializations_expose_each_result_when_a_project_file_is_moving` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_lifecycle_mcp.rs` | `a_read_continuation_does_not_depend_on_initialization_request_metadata` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_lifecycle_mcp.rs` | `a_scoped_inbox_keeps_outside_watched_threads_but_post_reads_stay_strict` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_lifecycle_mcp.rs` | `watched_thread_lifecycle_is_unread_and_continuations_are_reader_bound` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_scope_isolation.rs` | `a_nonselected_corrupt_watch_is_reported_beside_the_healthy_page` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_scope_isolation.rs` | `a_nonselected_real_watch_flag_is_refused_without_integer_coercion` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_scope_isolation.rs` | `a_nonselected_too_new_store_is_reported_beside_the_healthy_page` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_scope_isolation.rs` | `a_project_subscription_adds_foreign_context_after_a_plain_watch_leaves_it_uncovered` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_summary_membership.rs` | `acknowledgement_counts_but_unwatch_alone_does_not_create_summary_membership` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/inbox_summary_paging.rs` | `unread_summary_checks_beyond_a_full_foreign_first_page` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/intent_claim_race.rs` | `generated_role_replacements_and_claim_repeats_never_hand_one_cause_to_both_holders` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/intent_delivery_mcp.rs` | `explicit_intent_is_claimed_and_sent_as_a_stored_board_notification` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/intent_local_eligibility.rs` | `explicit_and_role_resolved_foreign_sessions_are_deferred_without_writes` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/late_facts_mcp.rs` | `close_is_terminal_and_late_facts_require_a_current_line_anchor` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_intent_sets.rs` | `deferred_human_commits_re_resolution_and_returns_the_added_local_intent` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `empty_role_is_terminal_for_this_cause_even_when_holder_returns` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `handed_off_recipient_stays_frozen_and_a_return_adds_no_second_handoff` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `named_target_wins_role_dedup_and_survives_role_removal` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `retained_claim_reinstates_removed_co_holder_under_its_original_identity` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `retained_holder_keeps_identity_updates_revision_and_adds_new_holder` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `returning_actor_reuses_original_intent_before_handoff` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `role_addressing_emits_exactly_zero_one_or_two_holders` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_intent_sets.rs` | `shrinking_set_supersedes_only_removed_actor_and_keeps_retained_identity` | OwnerFixture registers its workspace, board and project through the old central's session |
| `tests/integration/m2b_role_operand_costs.rs` | `unrelated_advisor_and_operator_holders_do_not_grow_write_operands` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_role_rules.rs` | `active_lead_cannot_be_moved_through_role_grant` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_role_rules.rs` | `many_roles_add_repeat_and_release_exactly_the_named_holder` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_role_rules.rs` | `workstream_grants_check_authority_before_distinctness_in_both_directions` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_subject_byte_budget.rs` | `byte_truncated_threads_and_subjects_are_not_decoded_until_returned` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_subject_read_costs.rs` | `distinct_subject_projection_has_size_independent_statements_and_decodes` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `every_subject_variant_is_required_and_read_titles_follow_current_records` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `missing_and_invalid_subjects_and_project_target_posts_are_refused` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `project_subject_and_refile_require_any_project_role_and_record_exact_history` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `row_subject_titles_follow_redaction_without_copying_private_text_into_thread` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `scope_checks_reject_foreign_records_and_cache_not_found_replays_after_arrival` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/m2b_thread_subjects.rs` | `session_role_witness_and_implicit_trail_use_their_own_scoped_operands` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/native_admission_gates.rs` | `actual_expiry_names_the_deadline_keeps_reads_and_same_epoch_renewal_admits_the_same_key` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/native_admission_gates.rs` | `an_actual_superseding_renewal_keeps_the_write_refusal_and_stops_public_reads` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/native_admission_gates.rs` | `an_unregistered_native_owner_refuses_the_first_write_and_registration_enables_it` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/operation_context_reads.rs` | `admitted_post_and_task_update_read_only_their_rule_operands` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/operation_context_reads.rs` | `semantic_identity_matches_survive_json_end_reencoding_and_typed_acknowledgement_readback` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/operation_context_reads.rs` | `stale_role_intent_reads_only_the_current_replacement_holder` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/participant_join_history.rs` | `handover_without_a_post_retains_the_advisor_and_promoted_intervals` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/participant_join_history.rs` | `role_change_without_a_leave_or_post_retains_each_join_interval` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_project_scope.rs` | `cold_corrupt_rejected_file_is_excluded_without_reading_it` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_project_scope.rs` | `rejected_creation_keeps_initialized_workspace_summary` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_project_scope.rs` | `rejected_creation_preserves_nonempty_repository_context` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_project_scope.rs` | `rejected_creations_leave_every_search_scope_and_archive_mode` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_views_mcp.rs` | `a_lead_decision_stays_settled_after_lead_replacement` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_views_mcp.rs` | `a_lead_with_a_pair_assignment_still_sees_lead_duties` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_views_mcp.rs` | `cards_show_the_current_governing_plan_and_empty_duties_without_inventing_a_role` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/pending_views_mcp.rs` | `pending_views_settle_linked_rows_and_redaction_changes_the_derived_answer` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/plain_watch_state.rs` | `first_thread_opt_out_survives_reopen_and_later_plain_project_watch` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/plain_watch_state.rs` | `watch_ledgers_replay_without_reappending_project_or_thread` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/post_cooldown_mcp.rs` | `top_level_cooldown_replays_remaining_time_and_does_not_cross_projects` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/post_page_mcp.rs` | `escaped_post_pages_traverse_every_record_under_the_frame_budget` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `a_directory_request_without_central_acceptance_creates_no_native_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `accepted_directory_survives_reopen_and_pending_registration_records_nothing` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `directory_refusal_keeps_its_structured_reason_without_creating_a_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `outbox_pruning_retains_pending_creation_until_registration_and_recovery_complete` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `ready_registration_recovery_uses_complete_input_and_the_creating_ledger_once` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `registered_creation_preserves_initial_state_and_replays_after_reopen_without_effects` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_creation.rs` | `synchronized_directory_rejection_releases_queued_creation_input` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description_encoded_sizes.rs` | `maximum_descriptions_survive_encoded_capture_and_populated_history_snapshots` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description.rs` | `a_directory_only_project_is_not_local_and_show_creates_no_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description.rs` | `current_and_feed_descriptions_are_strict_and_never_silently_truncated` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description.rs` | `descriptions_round_trip_through_create_describe_show_title_update_and_reopen` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_description.rs` | `omitted_and_explicit_empty_descriptions_share_the_creation_retry_hash` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_failure_isolation_mcp.rs` | `creation_recovery_contains_a_corrupt_sibling` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/project_failure_isolation_mcp.rs` | `healthy_thread_routing_survives_a_corrupt_sibling` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/push_surface/board_and_history_proofs.rs` | `control_dm_send_and_read_leave_board_messages_and_activity_empty` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/push_surface/board_and_history_proofs.rs` | `control_inbox_and_history_order_retained_dms_and_omit_expired_records` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/push_surface/board_and_history_proofs.rs` | `inbox_and_history_use_literal_short_preview_while_show_keeps_full_body` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/push_surface/subscription_expansion_proofs.rs` | `router_show_expands_only_stored_subscription_range_without_read_acknowledgement` | starts the old central (board-central-test-support) for its setup or calls |
| `tests/integration/reader_metadata_retention.rs` | `a_refused_foreign_project_subject_never_receives_this_projects_feed` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_retention.rs` | `link_events_count_once_for_the_source_task_and_its_workstream` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_retention.rs` | `removed_child_keeps_every_unread_entry_on_task_and_workstream` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_scopes.rs` | `scoped_native_selection_retains_every_thread_fact_and_intent_branch` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_scopes.rs` | `scoped_thread_metadata_keeps_off_subject_epoch_and_dense_position_checks` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_scopes.rs` | `task_workstream_and_project_metadata_follow_subject_relations_without_decoding_titles` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_scopes.rs` | `thread_metadata_skips_bodies_while_content_reads_refuse_corruption` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_metadata_scopes.rs` | `write_reader_counts_all_unread_but_returns_newest_fifty_and_pages_the_rest` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_state_corrections.rs` | `a_continuation_card_does_not_mark_its_subject_actor` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_state_corrections.rs` | `a_persisted_not_writer_refusal_hides_reads_without_inventing_a_successor` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_state_corrections.rs` | `anonymous_content_reads_have_no_mark_outcome_and_empty_threads_mark_nothing` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_state_corrections.rs` | `consecutive_content_reads_and_keyed_acknowledgements_never_feed_a_read_mark` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_state_corrections.rs` | `read_mark_failures_are_the_closed_write_gate_family` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_subject_metadata.rs` | `subtask_saved_decision_replays_after_text_body_corruption` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/reader_subject_metadata.rs` | `trail_saved_decision_replays_after_text_body_corruption` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/record_field_bounds_mcp.rs` | `trail_limits_preserve_maximum_values_and_replay_field_refusals` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/replication_view_progress.rs` | `acknowledged_inherited_prefix_keeps_the_epoch_that_assigned_its_rows` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/replication_view_progress.rs` | `acknowledgement_after_the_pinned_view_is_bounded_to_that_views_as_of` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/replication_view_progress.rs` | `native_mapping_failure_is_board_unavailable_instead_of_zero_progress` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/repository_locator_boundaries.rs` | `max_bound_escaped_locators_page_fully_below_the_frame_limit` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/repository_locator_boundaries.rs` | `oversized_stored_locator_is_rejected_by_domain_decoding` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/repository_locator_control_boundary.rs` | `oversized_repository_locators_return_field_and_numeric_bound` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/request_effects_public.rs` | `a_large_fleet_write_returns_a_signed_public_continuation` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/request_effects_public.rs` | `one_creating_request_has_distinct_project_and_fleet_receipts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_epoch_public_reads.rs` | `a_discarded_old_suffix_is_refused_even_when_new_epoch_reuses_its_position` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_epoch_public_reads.rs` | `range_predecessor_and_public_next_reuse_preserve_actual_assigning_epochs` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_epoch_public_reads/activity_reads.rs` | `retained_trail_search_inbox_and_stored_push_ranges_survive_a_later_writer_epoch` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_project_registry.rs` | `cancelled_native_close_stays_unavailable_until_the_same_drain_is_retried` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_project_registry.rs` | `completed_supersession_retries_without_reopening_the_closed_file` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/retained_project_registry.rs` | `snapshots_drain_before_swap_and_a_stale_read_handle_becomes_unavailable` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/role_authority_mcp.rs` | `lead_release_then_grant_cannot_bypass_the_owner_grant_or_reuse_it` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/role_authority_mcp.rs` | `workstream_pair_and_stand_down_acknowledgement_keep_authority_distinct` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/seeded_registry_restart.rs` | `a_pending_seed_stays_unpublished_after_registry_reopen` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/seeded_registry_restart.rs` | `a_seeded_file_cannot_be_opened_by_another_enrolled_peer` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/seeded_registry_restart.rs` | `persisted_expiry_is_not_revived_by_the_start_epoch_filename` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/seeded_registry_restart.rs` | `persisted_supersession_survives_the_registry_reopen` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/selection_failure_atomicity.rs` | `failing_second_membership_save_rolls_back_the_first_and_retries_without_restart` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/session_context_mcp.rs` | `context_covers_assignments_scope_facts_and_card_session_presence` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/subscription_unavailable_projects.rs` | `a_contained_project_is_reported_beside_exact_healthy_subscription_rows` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/subscription_windows.rs` | `local_selection_settles_without_changing_replicated_membership_or_feed` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_review_corrections.rs` | `actual_link_changes_have_events_but_noops_and_replays_do_not` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_review_corrections.rs` | `another_action_on_a_dropped_task_changes_no_domain_record` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_review_corrections.rs` | `dropping_requires_and_preserves_the_callers_reason` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_review_corrections.rs` | `initial_task_links_record_each_change_once_with_the_created_task` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_review_corrections.rs` | `removed_subtask_is_visible_in_history_and_only_explicit_removed_lists` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_rules_mcp.rs` | `concurrent_links_replace_and_trail_subject_rules_preserve_atomic_outcomes` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_rules_mcp.rs` | `cycle_drop_noop_and_subtask_lifecycle_keep_distinct_decisions` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/task_rules_mcp.rs` | `dependency_target_in_another_project_is_distinguished_from_missing` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/thread_reference_routing.rs` | `qualified_routes_and_bare_probes_never_open_another_store` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/thread_reference_subscriptions.rs` | `subscriptions_label_projects_and_qualified_roots_do_not_escape_them` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/thread_subscription_control_path.rs` | `subscription_methods_roundtrip_through_the_api_and_sqlite` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/trail_feed_mcp.rs` | `redacted_41_and_shown_42_keep_feed_coverage_and_descending_register_bounds` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/trail_page_budget.rs` | `escaped_trail_prefixes_preserve_forward_coverage_and_frozen_register_completion` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/trail_pending_rules_mcp.rs` | `handbacks_require_the_workstream_pair_and_owner_questions_name_a_blocked_record` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/unknown_time_windows.rs` | `unknown_imported_post_is_covered_and_only_the_later_live_post_opens_a_window` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/wait_renewals_mcp.rs` | `wait_renewals_replay_each_project_and_keep_partial_refusals_visible` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/watch_history_guidance.rs` | `history_guidance_exists_only_for_queryable_scoped_posts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/watch_history_guidance.rs` | `inactive_watch_hides_subscription_start_without_rewriting_history` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/watch_history_guidance.rs` | `watch_messages_explain_history_guidance_and_board_lifecycle` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/watch_read_lifecycle.rs` | `future_only_watches_scoped_acknowledgements_and_unread_summaries_survive_reopen` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/work_view_fact_bound.rs` | `public_work_operands_and_native_queries_ignore_growing_unrelated_facts` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |
| `tests/integration/work_view_mcp.rs` | `work_view_orders_dependencies_and_keeps_blockers_progress_and_closed_work_apart` | runs on BoardFixture's old-central board (enrolled Host, central-backed board) |

