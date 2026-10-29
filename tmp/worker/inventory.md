# W1 timing inventory

Source: 881f778c77b4d5532ef1be32f0bd2f44daaf73f5; generated before product edits.

Scope is the exact W1 list in Lead push 01a18721-cad9-7d81-8587-6e18cc86082f. W2, proxy, and verification TypeScript are excluded. The scan includes production timing owners so test dependencies are visible. Production scheduling remains unchanged unless an approved clock injection makes tests deterministic.

A = sleep/poll synchronization, B = real-duration behaviour, C = bound around causal wait. Initial ambiguous scan entries require source classification before conversion; timestamps used only as fixture data do not require elapsed time and are listed separately.

2089 occurrences across 833 grouped rows. Every comma-separated number is an original source line.

## Waits, scheduling, elapsed-time uses

Reviewed conversion entries (the broad scan below remains a coverage checklist):

| Source lines at base | Class | Plan |
| --- | --- | --- |
| crates/collaboration-service/tests/wakeup_repeating_delivery.rs:32,134,313 | B | Existing Clock injected in worker/sender through the same SubscriptionClock adapter; paused Tokio, committed first-delivery fence, retain 300-second minimum and both real native deliveries. |
| crates/collaboration-service/src/remote_wake_replay_tests.rs:131-137 | B | Set the already-injected recipient clock to frozen deadline plus 20 ms; keep the real SDK custody replay and public operation assertions. |
| crates/collaboration-service/src/subscription_delivery/tests/lifecycle_proofs.rs:181-186 | A | Diagnose owner checkpoint and native Drop settlement; replace incomplete wait with owning committed signal; retain both unread messages and one-message next push. |
| crates/collaboration-central-conformance/src/access_expiry_scenario.rs:42-45 | X | Lead exception 01a18724: real staging Access verifier clock is external. Keep only on staging journey; local callers must use the existing edge application clock. |
| crates/collaboration-central-conformance/src/access_scenarios.rs:12-30 | B | Local expiry helper takes the caller's existing clock advancement action; same signed bytes before/after. W2 owns its two callers. |
| crates/collaboration-mcp/src/api_test_harness.rs:309,349 | A | Observe registry admission and completion through a test-only watch; keep real listener and handler. |
| crates/collaboration-mcp/src/api_test_harness.rs:354 | A | Replace negative 200-ms read timeout with a held-call causal checkpoint and pending read assertion. |
| crates/collaboration-mcp/src/tool_call_registry.rs:166 | A | Prove running call at cancellation checkpoint, inspect pending ended future during shutdown grace, then abort and join. |
| crates/acp-client-runtime/tests/interaction_cancel_wire_order.rs:44,60 | A | Hold the interaction reply on a test-controlled channel after cancellation; verify cancel remains pending until released, then preserve exact wire order. |
| crates/codex-router-secret-store/src/account_credential_lock.rs:163 | A | Observe actual contention at the owning file lock; retain eventual acquisition after child release. |
| crates/codex-router-secret-store/src/encrypted_credential_store_tests.rs:110 | A | Observe actual store-lock contention before checking pending deletion; retain deletion after release and missing-secret readback. |
| crates/codex-router-host/src/external_provider_runtime/tests/mcp_shutdown_tests.rs:160,199,385 | A | Await actual prompt admission/context before overlapping, shutting down, or stale cancel; no fixed delay. |
| crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_tests.rs:369 | A | Wait for fixture PID/readiness via causal file observation, preserving the paused initialize timeout and reaped-process proof. |
| crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_tests.rs:507 | A | Wait for the active prompt/dispatch observation, then cancel. |
| crates/codex-router-host/tests/runtime_lifecycle.rs:230 | A | Advance paused timer under the existing owner boundary and retain exact health-request count; no idle delay oracle. |
| crates/codex-router-host/tests/automation_retention_runtime.rs:44 | A | Await the maintenance owner's committed prune pass, retaining instruction readback. |
| crates/codex-router-host/tests/wakeup_timer_composition.rs:35 | A | Use the existing first-fire causal wait through the client instead of interval-based reads. |

| Source lines | Classification and plan |
| --- | --- |
| crates/acp-client-runtime/src/agent_session_client/provider_connection_task.rs:708 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/src/agent_session_client/provider_frame_observation.rs:22 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/src/agent_session_client/provider_setting_application.rs:178 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/src/agent_session_client/submitted_prompt_tests.rs:69,89,106,129 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/src/agent_session_client/turn_admission_tests.rs:112,175,196,209,223 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/src/provider_session_actor.rs:488 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/create_settings_order/settings_tail_tests.rs:104 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/create_settings_order.rs:636 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/cursor_model_settings.rs:231 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/cursor_plan_approval.rs:158 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/cursor_question_roundtrip.rs:135 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/cursor_todo_events.rs:124,197 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/history_replay_order.rs:162 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/interaction_cancel_wire_order.rs:44,60 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/acp-client-runtime/tests/interaction_cancel_wire_order.rs:139,142,149 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_auth_status.rs:160 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_elicitation.rs:126 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_event_overflow.rs:150 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_idle_updates.rs:98,176 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_input_events.rs:128,157,169,218,236,519,529 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_multiblock_prompt.rs:137,182,206,240,260 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_turn_boundaries/failed_tool_plan.rs:20,45,68,72,106,139,148,159 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/acp-client-runtime/tests/provider_turn_boundaries.rs:94,117,142,165,169,199,203,216,227 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/src/board_commands/board_write_output_tests.rs:23 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/src/details_client_inspection_tests.rs:66,79 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/src/event_commands.rs:213 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/agent-collaboration/src/session_contract_tests/catalog_filter_tests.rs:163 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/agent-collaboration/tests/acp_bridge_command.rs:85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/admission_overload_cli.rs:114 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/admission_overload_cli.rs:126 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/automatic_summary_timeout.rs:53 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/busy_thread_delivery.rs:29,90 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/debug_backend_restart.rs:23,55 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/automation_live_support/debug_backend_restart.rs:74 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/durable_delivery_recovery.rs:140,259 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/automation_live_support/durable_delivery_recovery.rs:142 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/native_fork_continuation.rs:32 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/proof_context.rs:85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/automation_live_support/proof_context.rs:384,410 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/scheduled_continuity.rs:164 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/summary_failure_recovery.rs:58 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/automation_live_support/worker_timeout_proof.rs:28,69 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_e2e/central_archive.rs:46,102 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/central_joint.rs:71 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/central_pair.rs:70,209,240,253 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/complete_pages.rs:388 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/complete_pages.rs:397 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_e2e/fixture.rs:146,191,211 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/fixture.rs:198,219 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_e2e/follow_fixture.rs:163,182 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/mail_messages.rs:222,247 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/messaging_recovery.rs:110 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/messaging_recovery.rs:120 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_e2e/messaging_surface.rs:53 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/messaging_surface.rs:76 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_e2e/notification_peer.rs:161,182,217,224 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/owned_host.rs:85,232,272,396,472,473,485,528 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/records_views.rs:24 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/registration_orphan.rs:144 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_cancel_successor.rs:53,149,164 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_cancels.rs:293,543 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_controls.rs:341,419 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_copy_progress.rs:12,26 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_frontier_cut.rs:72 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_e2e/takeover_prefixes.rs:270,510 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_live_support/persistence_verification.rs:185 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/board_skill_dx_recovery.rs:121 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_skill_dx_support/mod.rs:260 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/board_write_uncertainty.rs:121 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/claude_background_cli/native_source_tests.rs:19,211 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/claude_background_cli.rs:160,201,265,297,321,350 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/conversation_fault_entry_paths/identity_override_tests.rs:28,180 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/conversation_fault_entry_paths.rs:95,194,307,466,647,659,791,798 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/conversation_fault_entry_paths.rs:639 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/debug_luna_acceptance.rs:115 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/debug_luna_acceptance.rs:149,166,178 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/agent-collaboration/tests/delivery_matrix/acp_target.rs:147,156,319 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix/acp_target.rs:180,293,395 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/delivery_matrix/approval.rs:125 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix/approval.rs:144 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/delivery_matrix/held_cells.rs:72 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix/held_cells.rs:102,318 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/delivery_matrix/native_push_receipt.rs:71 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix/public_push_receipt.rs:12 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/delivery_matrix/public_push_receipt.rs:20,21 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix/push_matrix_driver.rs:363,522,562 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/delivery_matrix/settled_creation.rs:14 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix_debug_acceptance.rs:436 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/delivery_matrix_debug_acceptance.rs:489 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/event_observation_commands.rs:264 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/event_observation_commands.rs:336,341 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/human_approver_cli.rs:332,342,366,458,472 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/local_mail_cli_mcp.rs:252 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/local_mail_failures_cli_mcp.rs:55,89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/mail_reconcile/connected_status.rs:45 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/mail_retirement/readback.rs:10,73 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/mail_retirement/wake_and_capacity.rs:35 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/messaging_skill_dx_support/inventory_evidence.rs:40 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/messaging_skill_dx_support/trace_evidence.rs:36 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/agent-collaboration/tests/native_bridge_command.rs:77,87 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/native_control_commands.rs:28,84,164 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/native_sandbox_access.rs:213 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/provider_inventory_source_reads.rs:53,63 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/provider_load_error_e2e.rs:196 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/provider_source_actions.rs:536,584 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/push_message_cli.rs:678 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/remote_mail_cli_mcp.rs:89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/skipped_wakeup_cli.rs:54 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/summary_skip_cli.rs:138 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/support/local_mail_capacity.rs:123,160 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/agent-collaboration/tests/support/local_mail_surfaces.rs:201 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/thread_subscription_cli.rs:617 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/wake_rules_cli_mcp.rs:56 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/agent-collaboration/tests/whoami_machine_identity.rs:127 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/automation-storage/src/schema_initialization.rs:215 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/access_edge.rs:170,291,296 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/enrollment_edge.rs:63,155,179,323,328 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/fixture_shutdown.rs:108 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/lib.rs:108,162 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/machine_enrollment.rs:60 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/machine_session.rs:117 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/mail_wire_observer.rs:152 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/owner_directory_observation.rs:16,44,63 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/owner_transfers.rs:42,62 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-central-test-support/src/piece_integrity_refusal.rs:88 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-fleet-sync/src/reqwest_session_transport.rs:64,76,104,137 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-fleet-sync/tests/fleet_service_session.rs:106,111,123,126,131,141,143,145 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-fleet-sync/tests/reqwest_session_transport.rs:108,211,247,254,317,325,332 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-fleet-sync/tests/support/cut_proxy.rs:33 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/src/import_access.rs:64 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/src/upgrade_prepare.rs:55,153,156,188,199,215 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/board-legacy-upgrade/tests/command_refusals.rs:292,463 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/command_refusals.rs:482 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/board-legacy-upgrade/tests/import_http_failures.rs:60,116 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/import_scaling.rs:83 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/import_scaling.rs:107 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/board-legacy-upgrade/tests/imported_claims.rs:46,75,295 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/message_cutover_cli.rs:55 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/message_cutover_crash.rs:35 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/message_report_guard.rs:16 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/seed_integrity_refusal.rs:88,89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/upgrade_singleton.rs:53 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/upload_publication.rs:205 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/upload_support/central_fixture.rs:168 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/board-legacy-upgrade/tests/upload_support/mod.rs:7 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/claude-code-peer-messaging/src/claude_code_peer_socket.rs:83,97 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/claude-code-peer-messaging/src/native_claude_job_reader.rs:87 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/claude-code-peer-messaging/src/native_claude_job_reader_tests.rs:217,228,239,249,253 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/src/acp_frame_transport.rs:52 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/src/native_prompt_execution.rs:383 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/tests/acp_connection_dispatch.rs:113,123,129,301,326,347,359,498,520,546,642 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/tests/cancellation_reload.rs:250 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/tests/native_prompt_execution/early_cancellation.rs:91,96 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/tests/native_prompt_execution.rs:419 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-acp-adapter/tests/prompt_detach.rs:112,268,296,444,550,616,661 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/acp_client_proof.rs:45,89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/acp_permission_proof.rs:80 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/acp_replacement_proof.rs:82,101,130,146 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/acp_replacement_proof.rs:109 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/examples/debug_host_acceptance/agent_tool_proof.rs:48 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/cli_event_listener.rs:41,69,111 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/cli_message_proof.rs:26 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/lifecycle_reader_proof.rs:66 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/owned_thread_registry.rs:240 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/owned_thread_registry_tests.rs:136,172,196 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance/recovery_observation.rs:25 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/examples/debug_host_acceptance/recovery_observation.rs:37,50,51 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/examples/debug_host_acceptance.rs:173,198 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/debug_host_acceptance.rs:264 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/examples/imagegen-debug-acceptance.rs:236,255,280,286 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/examples/imagegen-debug-acceptance.rs:244,265 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/examples/imagegen_debug_acceptance/native_proof.rs:233 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/acp_interaction_port.rs:163 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/acp_interaction_port.rs:611,616,622,644,649,655 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/app_server_endpoint_guard.rs:13 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/app_server_shutdown.rs:185,201 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/auth_file_reload.rs:74,131 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/automation_settings_crash_tests.rs:31 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/codex_update_preparation.rs:129,160,173,187,203 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/central_read_copy.rs:211,266 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/machine_session.rs:293,302,309 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/collaboration_runtime/board_composition/project_installation.rs:179 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/project_writer.rs:349,378 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_observation.rs:16,64,125,160 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_owner_replacement.rs:202,223,337 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/archive_sync_barrier.rs:17,46,54,83 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/attaching_server_calls.rs:23,146,157,160,217,230 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_directory_expiry.rs:19,139,181 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copies.rs:81,133,157,186,236,331,344 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copy_restart.rs:15,64,111,119,157,209,254 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copy_schema.rs:264 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_cli.rs:13,77 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_fixture.rs:87 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_lost_grant.rs:42,52,58 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_notice_overlap.rs:93,118,182,209,223,234,237,248,256,260,261 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_refusals.rs:149,150,167 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_renewal_order.rs:84,115,163,202,228,242 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/claim_restart.rs:79,103,194,229,237,310,349 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/fleet_service.rs:525,582 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_notification_fixture.rs:69 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_notifications.rs:88,132,134,139,177,191,202 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_sender.rs:394 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/forwarded_post_unknown.rs:53,68 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/importer_claim.rs:88,139,176 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_epoch_transfer.rs:23,55,116 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_events.rs:174,179,194,211 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_harness.rs:178,182,211,383,400,447 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_harness.rs:390 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_owner.rs:244 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_relay_paths.rs:13,65,352 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_crash.rs:134 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_frontdoor.rs:183,205,324 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_remote_restart.rs:86 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_session_transport.rs:245 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_status_catch_up.rs:40 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_submission_epochs.rs:16 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_cancel_cases.rs:155,208,233,278,306,514 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_cancel_fault_cases.rs:36,79,92,145,201 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_inherited_cancel.rs:128,154 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_local_cancel_case.rs:109,146 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_cases.rs:162 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_fixture.rs:143 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/messaging_rule_publication.rs:18,83,115 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/non_holder_task_writes.rs:45,62,99,125 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_acknowledgement.rs:48,124,137,151,159,166,177,293,303 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_reaction_dispatch.rs:58,102,115,130 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_reactions.rs:79,89,92 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_authority_recovery.rs:64,240,261,274,285,304,353 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_directory.rs:31,46 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_frontier.rs:23,82,113 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_owner.rs:218 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_prefix.rs:60,61,87,127,150 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_project_fixture.rs:68,88,92,94,95,170 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_public_controls.rs:74,97 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_publisher.rs:134,139,164,181 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_remote_cancellation.rs:120,180,199,238,248,257,295 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_replication.rs:46,64,97,132 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_retirement_crash.rs:103,164,225 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_welcome.rs:84,108,155 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/persisted_supersession.rs:106 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/project_claim.rs:163,198 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/project_creation_replies.rs:42,48,50,66,85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/project_integrity_refusal.rs:86,141 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/read_copy_page_gate.rs:56 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/received_grants.rs:37,69,158,243,274,340,354,359,371,384,463,537 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/returning_holder_expiry.rs:72,82,89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/snapshot_install_child.rs:64,118,125,131,201,219,250 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/snapshot_install_graceful.rs:91,153,155,210,293,306,356 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests.rs:131,211,295,306,347,350,352,485,495,510,517 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/collaboration_api_serving.rs:159,163 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/provider_retention.rs:32 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/collaboration_runtime/startup_contract_tests/broker_context_tests.rs:52 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/startup_contract_tests/follow_attachment_tests.rs:41,76 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/collaboration_runtime/startup_contract_tests/wake_capacity_tests.rs:79,90 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/debug_readiness_timing.rs:9 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/explicit_app_server_restart.rs:53 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/explicit_router_restart.rs:121 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/external_provider_runtime/approval_dispatch_tests/permission_outcome_tests.rs:124,172,174,246,250,260 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/approval_dispatch_tests.rs:148,480,497,602,659,682,726,782,788 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/live_tests.rs:237,243,413,502,645 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/live_tests.rs:380,476,649 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/external_provider_runtime/robustness_tests.rs:80,144,162,190,250,282,419,495 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_tests.rs:334,337,448 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_tests.rs:369,507 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/external_provider_runtime/tests/mcp_shutdown_tests.rs:160,199,385,548 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/external_provider_runtime/tests/mcp_shutdown_tests.rs:239,251,291,339,554 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_runtime/tests.rs:45,112,119 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_supervisor/provider_conversation_backend.rs:518 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_supervisor/provider_delivery_submission.rs:95 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_supervisor/tests.rs:347,560 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/external_provider_supervisor.rs:434,442,457 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/host_singleton_authority.rs:198 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/lifecycle_convergence.rs:69,329 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/request_admission.rs:552 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/lifecycle_owner/request_admission.rs:564 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs:84 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs:137 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs:381 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/startup_diagnostic_tests.rs:115 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/lifecycle_owner/status_observation.rs:190 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/tailnet_failure_tests.rs:32 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner/update_activation.rs:81,144 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/lifecycle_owner/update_activation.rs:159 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/lifecycle_owner.rs:368,558,597,750 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/lifecycle_owner.rs:386 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/managed_app_server.rs:264 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/managed_app_server.rs:308 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/operator_connection.rs:37,41,51,79 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/owned_router.rs:70 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/provider_acp_message_fifo.rs:415 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/src/provider_acp_message_fifo_tests.rs:209,400 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/provider_session_command_port.rs:501 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/provider_session_event_sink/lifecycle_tests.rs:206,228 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/provider_session_event_sink.rs:404,467,519,565,710 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/router_compatibility.rs:46,73,81 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/router_compatibility.rs:54,66 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/src/router_executable_observation.rs:132 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/tailscale_exposure/mcp_readiness.rs:67,69,248 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/tailscale_exposure/mcp_readiness_tests.rs:94,117,226 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/tailscale_process/tests.rs:118,125 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/src/tailscale_process.rs:81,232,272,284 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/acp_metadata_projection.rs:98 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/auth_file_reload.rs:176,190 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/automation_retention_runtime.rs:43 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/automation_retention_runtime.rs:44 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/tests/claude_code_peer_scheduled_runs.rs:461,477 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/claude_peer_held_dm.rs:151,201,215,267,313 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/claude_peer_subscription_resume.rs:49,85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/collaboration_runtime.rs:343 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/external_provider_composition.rs:260 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/external_provider_recovery.rs:176 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/external_provider_supervisor/lifecycle_tests.rs:218,303,351,441,451,457 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/external_provider_supervisor/settings_projection_tests.rs:80 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/external_provider_supervisor.rs:208,303,309,407 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/host_replacement_failures.rs:152,260,265,440,455 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/native_claude_catalogue.rs:21 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/process_lifecycle.rs:210,426,511,530,606,624 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/production_enrollment.rs:195,275,396 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_delivery_route/auto_direct_message_tests/uncertainty_push_tests.rs:103 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_delivery_route/auto_direct_message_tests.rs:136,151,159,169 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_delivery_route/provider_failure_tests.rs:19,59 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_delivery_route.rs:199,261,559 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_message_fifo.rs:95,161,251,349,492,600,645,806 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_message_fifo.rs:336 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/tests/provider_acp_queue_route.rs:222 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_scheduled_runs.rs:447,485,491 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_acp_steering_route.rs:250,275,362 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_content_commands.rs:231 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_face_composition.rs:98,120,150,249,279,295,323,368,385,401,426,455 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/provider_pending_hub_races.rs:230,258,331 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/remote_wake.rs:123,140,158,214,217,226,270,292 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/runtime_lifecycle.rs:230 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/tests/runtime_lifecycle.rs:374,441,487 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/runtime_restart.rs:207,262,338 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/runtime_restart.rs:219 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/tests/singleton_authority.rs:321 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/claude_peer_subscription_clock.rs:61 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/tests/support/claude_peer_subscription_clock.rs:112 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/claude_peer_subscription_fixture.rs:344 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/claude_peer_subscription_transport.rs:31 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/operator_client.rs:21,41,46,83 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/operator_client.rs:32 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-host/tests/support/registered_route_project.rs:72 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/runtime_peer_registry.rs:445 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/runtime_peer_registry_cleanup.rs:21,146 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/session_message_approval_fixture.rs:162,180 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/session_message_route_fixture.rs:39,90,282,298 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/support/update_reexec_fixture.rs:274,409,427,440 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/update_reexec.rs:143,159,383,461 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/update_reexec.rs:152 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-host/tests/wakeup_timer_composition.rs:34 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-host/tests/wakeup_timer_composition.rs:35 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-secret-store/src/account_credential_lock.rs:160,163,169 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-secret-store/src/central_credential_tests.rs:159 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-secret-store/src/credential_migration_tests.rs:618,644 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-secret-store/src/encrypted_credential_store_tests.rs:107,110,116 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-secret-store/src/file_backend/debug_plaintext_tests.rs:329,338 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/codex-router-state/src/sqlite/policy_mutation.rs:173,222 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/codex-router-state/src/state_tests/floor_mutation_tests.rs:214 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/codex-router-state/src/state_tests/floor_schema_tests.rs:277 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/examples/support/workerd_process.rs:50,81,82 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/access_expiry_scenario.rs:45 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-central-conformance/src/composed_client.rs:57,101 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/composed_numeric.rs:228 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/conformance_client.rs:23,39 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/enrollment_scenarios.rs:211 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/holder_reply_scenarios.rs:98,179,205 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/lease_session_scenarios.rs:76 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/mail_status_scenarios.rs:29 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/native_board_session.rs:33 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/normal_access_scenarios.rs:106,141 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/readmission_scenarios.rs:187,292 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/revocation_race_scenarios.rs:86,170,202 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/revocation_race_scenarios.rs:176 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-central-conformance/src/runtime_control_scenarios.rs:128,398 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/runtime_faults/selected_call_tests.rs:41,42,43,45,56,68,75,76,78,86,87,97,102,181,246,252,265,270,328,348,396,433,442 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/session_replacement_scenarios.rs:46,75 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-central-conformance/src/transaction_reopen.rs:138,214 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/api_test_harness.rs:307,347,355 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/api_test_harness.rs:309,349 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-mcp/src/carrier_response_loss_http_tests.rs:139,410,435 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/codex_conversation_cancel_http_tests.rs:51 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/collaboration_api_router.rs:191,195,203,208 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/collaboration_api_router_tests/call_lifecycle.rs:157 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/mcp_remediation_tests.rs:45 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/mcp_server/carrier_tools.rs:271,332,358 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/mcp_server/carrier_tools.rs:322 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-mcp/src/mcp_server/carrier_tools.rs:340,368,395 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-mcp/src/observation_streaming_tests.rs:148,285 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/permission_entry_path_tests.rs:322,336 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/provider_conversation_http_tests.rs:622 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/provider_conversation_live_http_tests.rs:16 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/provider_stdio_diagnostic_http_tests.rs:150 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/proxy_authority_tests.rs:30,146 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/tool_call_registry.rs:113,114,134,156,158,182 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-mcp/src/unix_origin_tests.rs:26,53,56 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/acp_channel_listener.rs:201 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/automation_retention_worker.rs:83 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/automation_retention_worker.rs:177 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/codex_app_server_delivery_route.rs:110,341 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/codex_app_server_scheduled_runs.rs:164 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/codex_queue_reconciliation.rs:59 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/interaction_broker/native_approval.rs:168,191 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-runtime/src/interaction_broker/native_history_fault_tests.rs:36 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/interaction_broker/turn_cancellation_tests.rs:138 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/interaction_broker/typed_interaction_notice.rs:194 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/interaction_broker/typed_interaction_notice.rs:248 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/interaction_broker_tests.rs:106 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/native_channel_relay.rs:89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/native_message_dispatch.rs:57,250,263,372,551,639 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/native_thread_preparation.rs:103,117 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/provider_session_event_hub.rs:601 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/push_handoff_worker.rs:47 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/router_session_app_server_multichoice_tui_tests.rs:117,189 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/router_session_app_server_multichoice_tui_tests.rs:118,122,134,144,184 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/router_session_app_server_order_tests.rs:18,102 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/router_session_app_server_reattach_tests.rs:75,81,115,121,259,273,375,394,411,417,487,500,571,579,731 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/router_session_app_server_tests.rs:601,607,630,636 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/router_session_app_server_tui_tests.rs:216,218,318,322,334,342,370,543,547 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/router_session_app_server_tui_tests.rs:222,317,374,464,468,551 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/schedule_timing_worker.rs:35 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/schedule_timing_worker.rs:95 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/scheduled_input_validation_tests.rs:185 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/scheduled_native_dispatch.rs:224 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/scheduled_submission_outcome.rs:120 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/summary_deadline_tests.rs:425 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/summary_native_worker.rs:86,455 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/summary_timeout_crash_tests.rs:45,148,205 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-runtime/src/wakeup_timing_worker.rs:30 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-runtime/src/worker_timeout_crash_tests.rs:48,322,368 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/collaboration_application/board_operations.rs:396 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/collaboration_application/conversation_operations.rs:416 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs:76 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs:119 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/collaboration_application/wake_operations/remote.rs:283,412 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/collaboration_application/wake_operations.rs:436,463,491 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/collaboration_application/wake_operations.rs:511 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/frozen_mail_query_fixture.rs:233 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/frozen_remote_cancel_tests.rs:50 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/local_cancel_crash_tests.rs:44 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/local_mail_admission_tests.rs:338,342 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/local_mail_interruption_tests.rs:19,48 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/mail_delivery_runner_probe.rs:216,226,233,240 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs:188 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs:408 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_delivery_crash_tests.rs:63,137,206,285,291 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_delivery_crash_tests.rs:138 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/native_generic_queue_crash_tests.rs:14,132 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_loaded_permission_tests.rs:43,49,55,123,130,137,223,231,234,282,290,293,354,361,368,425,432,439 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_policy_preservation_tests.rs:26,34,37,90,107,110 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_policy_queue_tests.rs:219,287 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_policy_stored_run_tests.rs:125,163,168 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_push_handoff_crash_tests.rs:13,83,99 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_push_handoff_crash_tests.rs:84 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/native_recorded_classification_tests.rs:104,130,133 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/native_recorded_permission_tests.rs:296,322 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/project_admission/project_creation.rs:121 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/project_mail_crash_tests.rs:59 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/remote_direct_send_tests.rs:75 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/remote_mail_assembly_tests.rs:86 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/remote_wake_replay_tests.rs:133 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/subscription_delivery/intent_progress_tests.rs:220,381 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/intent_role_delivery_tests.rs:340 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/reader_delivery_actions.rs:163 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/subscription_delivery/reader_delivery_owner.rs:465 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/subscription_delivery/subscription_clock.rs:8 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/subscription_delivery/subscription_clock.rs:12 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/subscription_delivery/subscription_push.rs:283 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/subscription_delivery/subscription_startup_recovery.rs:166 | C or B: inspect deadline owner; retain causal guard, inject behavioural deadline |
| crates/collaboration-service/src/subscription_delivery/tests/activity_scope_proofs.rs:139 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/control_view_proofs.rs:48,106,184,202 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/direct_message_proofs.rs:48,56,316,536 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/f6_held_record_proofs.rs:53,80 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/flight_proofs.rs:15,73,186 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/incoming_poll_session.rs:107 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/lifecycle_proofs.rs:42 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:148,168,212,218,239,298,329,403,426,433,480,487,505,525 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_activation_frontier.rs:47,155,175,220 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_cancel_backpressure_proofs.rs:47,248 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_cancel_proofs.rs:33,84,140,226,336 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_expiry_proofs.rs:127,186,256,272,319,330 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_grouping_proofs.rs:13,63,127,218 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_injection_proofs.rs:30,60,96,160,295,304,309,316,321,324 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_mail_proofs.rs:111,189 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_follow_outcome_fault_proofs.rs:25,36,144,154,218,237,290,302 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_frontier_proofs.rs:52,121,183,239,247,358,366,400 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_generation_isolation_proofs.rs:59,83,107,126,155,221,227,256,347,376,386,412 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_generation_prevention_proofs.rs:19,47,145,175,193,207 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_handoff_recovery_proofs.rs:65 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_held_mail_proofs.rs:25,61,74,93,128,140,143,151,158 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_intent_publisher_proofs.rs:104,195,221 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_mail_attempt_proofs.rs:138,144,233 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_owed_transfer_proofs.rs:50,85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_poll_gate_proofs.rs:44,59,165,184 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_poll_incoming_proofs.rs:92,112,126 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_poll_mail_proofs.rs:31,68,73,173,216 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_poll_recovery_proofs.rs:15,49,88,167,254,287,318,322,356 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_readiness_interleaving_proofs.rs:55,117,131,154,172,179 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/native_successor_frontier_proofs.rs:244,314,428 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/owner_activity_proofs.rs:10 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/owner_clock.rs:16 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/owner_clock.rs:81 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/subscription_delivery/tests/owner_fixture.rs:430 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/owner_native_lifecycle.rs:12,58,81 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/push_store_proofs.rs:114,147,157,428,473 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/reconcile_retry_proofs.rs:46,55,57 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/restore_corruption_proofs.rs:81,233,394,418 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/stall_diagnosis.rs:22,32,47,116,126 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/startup_fixture.rs:21,78 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/startup_recovery_regressions.rs:156,245,312 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/startup_retry_proofs.rs:126,247 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/startup_retry_proofs.rs:185 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/src/subscription_delivery/tests/subscription_observation_proofs.rs:46,88,94,116,131,139,149,167,192,200,209,214,247,285,309,376,411,489 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/timing_proofs.rs:104,128,134,165,188,200 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/transient_scan_proofs.rs:67,127 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/subscription_delivery/tests/wait_proofs.rs:268,323 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/src/wake_creation_crash_tests.rs:35 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/acp_listener_admission.rs:71,82,112,136,156,183,199,210,229,247,263,272,280,291,299,308,325,334,343,351,437,450,458,469,480,487,495,515,528,538,546,553 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/acp_listener_socket_cleanup.rs:64 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/acp_provider_interactions.rs:32,210,221,271,375,412 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/app_server_client_rejection.rs:119,144,240,258 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/app_server_provider_interactions.rs:147,155,169,211,240,259,272,288,295,309,349,376,390,396,411,419,508,519,548,599 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/board_concurrency_behaviors.rs:215,238,249,259 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/board_control_path.rs:146,149,207,220,240,280 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/board_lookup_failures.rs:146 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/board_search_behaviors.rs:431,456 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/board_write_scale.rs:152,158 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/bounded_membership_reads.rs:69 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/capture_feed_marks.rs:130 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_app_server_delivery_route/generation_tests.rs:413,640,642 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_app_server_delivery_route.rs:295,297,351,395 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_app_server_held_thread_route.rs:279,341,385 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_app_server_wake_push_route.rs:177,186,247 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_detached_create_delivery.rs:59,189,274,287,292 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_empty_thread_recovery.rs:89 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/codex_scheduled_existing_targets.rs:103 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/held_queue_depth.rs:123 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/intent_delivery_mcp.rs:222,288,369,397 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/journal_rpc.rs:38,48 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/native_channel_relay.rs:63 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/native_empty_session_inventory.rs:108 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/native_listener_recovery.rs:27,60,70 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/native_relay_handshake.rs:50,63,73,140,151,166,179 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/project_store_registry.rs:88 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/provider_session_observation_control.rs:110,262,482 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/public_observation_recovery.rs:86,116 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/public_process_recovery.rs:47,66 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/replication_view_progress.rs:244 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/retained_project_registry.rs:119,167,223 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/run_reconciliation_path.rs:236,275 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/schedule_native_preparation.rs:201 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/scheduled_run_path.rs:335,345,452 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/scheduled_run_path.rs:453 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/tests/support/acp_provider_interactions_remediation.rs:251 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/support/alarm_read_costs.rs:177 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/support/board_fixture.rs:242,246,251,269,275 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/support/native_mail_owner.rs:148 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/support/write_family_timing.rs:56 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/collaboration-service/tests/test_support/message_backend_fixture.rs:351,384,427,434,480 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/wait_renewals_mcp.rs:19 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/wakeup_first_fire.rs:51 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/wakeup_native_delivery.rs:386,398,406,489 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/collaboration-service/tests/wakeup_native_delivery.rs:399 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/collaboration-service/tests/wakeup_repeating_delivery.rs:210,283,287,309,376,446 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/interaction-storage/src/held_mail_readiness_tests.rs:103,106 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/interaction-storage/src/interaction_history_sqlite_lifecycle_tests.rs:285 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/interaction-storage/src/push_records.rs:54 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/interaction-storage/tests/push_repository.rs:505 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/lifecycle-observation/src/lifecycle_store.rs:165,352,394 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/lifecycle-observation/src/lifecycle_store.rs:198 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/lifecycle-observation/tests/journal_long_poll.rs:33,42 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/lifecycle-observation/tests/native_stream_reconciliation.rs:85 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/message-board-storage/src/project_crash_tests.rs:47 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/message-board-storage/src/project_import_batch.rs:48 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |
| crates/message-board-storage/src/project_store_tests.rs:423 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/tailnet-verification-host/src/bounded_command.rs:49 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/tailnet-verification-host/src/host_lifecycle.rs:121,183,199,226 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/tailnet-verification-host/src/host_lifecycle_interrupt_tests.rs:38,177 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/tailnet-verification-host/src/host_lifecycle_interrupt_tests.rs:71 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/tailnet-verification-host/src/mcp_observation.rs:96 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/tailnet-verification-host/src/owned_host.rs:196 | A or B: inspect owner; replace polling with signal or drive injected time |
| crates/tailnet-verification-host/tests/mcp_transport.rs:111,174 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/tailnet-verification-host/tests/owned_cleanup.rs:40,207 | C: retain bound around causal wait; inspect negative timeout assertions for B |
| crates/workspace-architecture-check/src/workspace_scan.rs:94,157 | B or diagnostic: inspect elapsed assertion; replace timing oracle with causal state |

## Timestamp and deadline origins

These are context candidates, not automatically violations: inspect the consumer; replace behavioural wall time with injected/fixed time, retain timestamp data that never needs time to pass.

| Source lines | Plan |
| --- | --- |
| crates/agent-collaboration/src/event_commands.rs:207 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/src/session_contract_tests/catalog_filter_tests.rs:148 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/automatic_summary_timeout.rs:52,92,149 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/busy_thread_delivery.rs:89,206 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/debug_backend_restart.rs:73,109 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/durable_delivery_recovery.rs:141,166,200,214,281 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/native_fork_continuation.rs:31,42,92 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/proof_context.rs:383,391,409,454 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/scheduled_continuity.rs:81,163,249 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/summary_failure_recovery.rs:57,116,160,182 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/automation_live_support/worker_timeout_proof.rs:68,146 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/complete_pages.rs:38 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/fixture.rs:75 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/installation_directory_pair.rs:93,111 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/mail_migration.rs:74,181,183 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/messaging_expiry.rs:11 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/owned_host_startup_proofs.rs:13 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/record_inputs.rs:71 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/records_views.rs:348,352 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_e2e/rejected_scopes.rs:136 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_skill_dx_recovery.rs:120,134 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/board_skill_dx_support/mod.rs:259,278 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/debug_luna_acceptance.rs:114 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/acp_target.rs:280,290,385,392 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/approval.rs:143,161 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/held_cells.rs:68,138,317,342 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/owned_resource_cleanup_readback.rs:66 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/public_push_receipt.rs:14,22 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix/push_matrix_driver.rs:362,379,521,545,561,588 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/delivery_matrix_debug_acceptance.rs:488,495 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/human_approver_cli.rs:341,456,470 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/messaging_skill_dx_support/inventory_evidence.rs:39,47 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/messaging_skill_dx_support/trace_evidence.rs:35,43 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/public_push_receipt.rs:43,60,64,75,90 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/push_delivery_matrix.rs:317,325,519,524,558,563 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/remote_mail_cli_mcp.rs:384,469 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/support/local_mail_capacity.rs:89 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/support/local_mail_capacity_seed.rs:130 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/agent-collaboration/tests/thread_subscription_cli.rs:526 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/automation-storage/src/schema_initialization.rs:85 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/automation-storage/tests/wake_slot_upgrade.rs:61,100,102,154,220,289 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/board-legacy-upgrade/src/import_access.rs:31 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/board-legacy-upgrade/src/upgrade_prepare.rs:53,150,154,167,190,200 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/board-legacy-upgrade/tests/import_scaling.rs:82 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-acp-adapter/src/native_prompt_execution.rs:46 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-acp-adapter/src/session_creation.rs:599 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-acp-adapter/tests/native_prompt_execution.rs:261 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-acp-adapter/tests/stored_session_listing.rs:11 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/examples/debug_host_acceptance/recovery_observation.rs:17 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/examples/debug_host_acceptance.rs:144 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/examples/imagegen-debug-acceptance.rs:343 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/automation_settings_file.rs:170,260 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/claude_code_peer_scheduled_runs.rs:232 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/archive_observation.rs:103 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/machine_session.rs:294,303 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/central_read_copies.rs:32 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/directory_inputs.rs:41 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/fleet_service.rs:118,159 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/importer_claim.rs:167 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/installation_directory_refresh.rs:89,96 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_harness.rs:382 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notice_reactions.rs:215 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_fleet_probe.rs:74,88 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/project_claim.rs:67,206 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/board_composition/tests/received_grants.rs:60 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/native_backend.rs:13,102,114 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/provider_retention.rs:38 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime/startup_contract_tests/wake_capacity_tests.rs:52 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/collaboration_runtime.rs:275,301 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/explicit_app_server_restart.rs:51 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/explicit_router_restart.rs:113,115,123 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/external_provider_runtime/tests/admission_lifecycle_tests.rs:362,363 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/external_provider_supervisor/provider_conversation_backend.rs:504 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/external_provider_supervisor.rs:737 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/host_replacement_activation.rs:35 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/lifecycle_owner/collaboration_lifecycle.rs:40,122,142,171,236 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/lifecycle_owner/request_admission.rs:133,155,176,309,362,423,442 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/lifecycle_owner/startup_convergence.rs:17,36,129,131,139,152,153 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/lifecycle_owner/update_activation.rs:158 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/lifecycle_owner.rs:204,224,267,700 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/managed_app_server.rs:259 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/provider_operation_settlement.rs:40,49,129 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/provider_queue_operation_registry.rs:284 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/provider_startup_composition.rs:261 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/src/router_compatibility.rs:44 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/board_failure_isolation.rs:22,202 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/claude_peer_held_dm.rs:56 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/external_provider_recovery.rs:285,293 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/provider_acp_delivery_route/auto_direct_message_tests.rs:47 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/provider_acp_scheduled_runs.rs:81 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/provider_pending_hub_races.rs:227,256 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/remote_wake.rs:244 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/runtime_restart.rs:196 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/support/claude_peer_subscription_clock.rs:18,96 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/support/claude_peer_subscription_fixture.rs:110 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/support/operator_client.rs:19,29,33 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/support/session_message_route_fixture.rs:277 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/update_reexec.rs:141 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-host/tests/wakeup_timer_composition.rs:30 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-secret-store/src/account_credential_lock.rs:116,118,146,148 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-secret-store/src/central_credential_tests.rs:150,156 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-secret-store/src/file_backend/debug_plaintext_tests.rs:120,131,272,284,306,322,405,414 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-state/src/account_migrations/tests/fixtures.rs:468,471 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-state/src/account_migrations.rs:117,119 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-state/src/sqlite/policy_mutation.rs:148,151,167,174,197,200,216,223 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/codex-router-state/src/state_tests/floor_mutation_tests.rs:206 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-cloudflare/src/object_shell/conformance_routes.rs:35 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-cloudflare/src/object_shell.rs:35 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-cloudflare/src/platform.rs:28 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/access_expiry_scenario.rs:40,42 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/access_fixture.rs:64 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/access_scenarios.rs:22 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/archive_runtime.rs:96 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/composed_copy_suite.rs:176 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-central-conformance/src/runtime_control_scenarios.rs:259 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-mcp/src/mcp_server/carrier_tools.rs:234,315,347,374 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-protocol/tests/board_notification.rs:23 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/app_server_event_forwarding.rs:73,238,311,351 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/automation_retention_worker/interaction_history_retention_tests.rs:295 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/automation_retention_worker.rs:87,153,270,320 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/codex_app_server_delivery_route.rs:105,339 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/codex_conversation_operation_recorder.rs:112,143,165,176,224 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/delivery_reconciliation.rs:114 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker/native_approval.rs:92,118,130,163 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker/typed_interaction_notice.rs:113,161,239 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker/typed_interactions.rs:33 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker.rs:310,770 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker_tests/decision_history_tests.rs:6,184,222,238,368 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker_tests/question_lifecycle_tests.rs:471 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/interaction_broker_tests.rs:577,635 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/mail_push_preparation_tests.rs:115 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/native_message_dispatch.rs:49,164,255,363 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/native_thread_preparation.rs:102 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/received_mail_push_tests.rs:107,138,140,169,206,247,274,287 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/router_session_app_server_multichoice_tui_tests.rs:126,132,176,178 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/router_session_app_server_tui_tests.rs:204,213,326,332 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/run_reconciliation.rs:134 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/schedule_preparation_evidence_sink.rs:166 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/schedule_timing_worker.rs:109 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/scheduled_run_evidence_sink.rs:64,137 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/scheduled_run_retry_tests.rs:278 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/scheduled_run_worker.rs:191,275,448,621 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/scheduled_submission_outcome.rs:94 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/stored_inventory_observation.rs:33 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/summary_deadline_tests.rs:52 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/summary_native_worker.rs:57,121,143,175,287 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/wakeup_delivery_sender.rs:135,224,269 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/wakeup_delivery_sender_tests.rs:66,151 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-runtime/src/wakeup_timing_worker.rs:49 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/codex_session_inventory.rs:140,279,592,652 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/automation_history_operations.rs:86,221 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/automation_operations.rs:86,203,222,320 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/board_view_operations.rs:58 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/conversation_operations.rs:390,403,450,455 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/journal_operations.rs:94 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/message_operations.rs:114,198,285 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/provider_observation_operations.rs:72 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/schedule_operations.rs:62,123,183,287 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/wake_operations/remote.rs:34,242,243,265,328 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/collaboration_application/wake_operations.rs:80,139,212,336,427,505,512 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/mail_delivery_runtime/remote_wakes.rs:397 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/mail_push_delivery_fixture.rs:144 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/native_delivery_crash_tests.rs:158,249,354 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/native_generic_queue_crash_tests.rs:62,75 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/native_policy_queue_tests.rs:46 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/native_push_handoff_crash_tests.rs:116 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/native_recorded_classification_tests.rs:165 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/project_admission/archive_observation.rs:73 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/project_admission/behavior_tests/subscription_fixture.rs:105 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/project_admission/intent_admission.rs:142 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/project_admission/project_creation.rs:153 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/project_admission/read_mark_admission.rs:84 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/provider_session_inventory.rs:215,380,393 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/remote_wake_replay_tests.rs:30,132,135,147,200,247,265 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/schedule_preparation.rs:155 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/scheduled_history_retention_tests.rs:30 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/startup_constructor_tests.rs:129 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/subscription_clock.rs:21,24 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests/local_mail_proofs.rs:133 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests/owner_clock.rs:41,54 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests/owner_setup.rs:55 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests/stall_diagnosis.rs:115,121 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests/startup_retry_proofs.rs:183,186 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/subscription_delivery/tests.rs:25,28 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/wakeup_delivery_sender/aged_wake_retention_tests.rs:177 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/src/wakeup_subscription.rs:31,125 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/authority_and_fact_corrections.rs:124 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/board_concurrency_behaviors.rs:213,216,247,250 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/board_control_path.rs:139,144,166,214,238,243 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/board_lookup_failures.rs:124 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/board_search_behaviors.rs:422,435 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/board_write_scale.rs:150,156 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/bounded_membership_reads.rs:126 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/bounded_window_reads.rs:185 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/codex_app_server_wake_push_route.rs:69 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/codex_conversation_operation_inspection.rs:135 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/context_scope_corrections.rs:129 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/covered_read_marks_mcp.rs:61 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/fleet_lifecycle_mcp.rs:200,309,333,340 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/held_queue_depth.rs:121 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/historical_delivery_receipts.rs:37 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/native_admission_gates.rs:270 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/native_control_calls.rs:209 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/pending_project_scope.rs:204,292 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/push_surface/dm_read_proofs.rs:238 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/push_surface/subscription_expansion_proofs.rs:259 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/reader_metadata_scopes.rs:256 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/retained_epoch_public_reads/activity_reads.rs:353 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/schedule_native_preparation.rs:164 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/scheduled_run_path.rs:405 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/selection_failure_atomicity.rs:29,78 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/session_context_mcp.rs:41 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/subscription_windows.rs:63,81 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/support/write_family_timing.rs:54 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/thread_subscription_control_path.rs:209 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/unknown_time_windows.rs:41,127,199 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/wakeup_first_fire.rs:64 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/wakeup_native_delivery.rs:118,184,439 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/wakeup_repeating_delivery.rs:104,128 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/collaboration-service/tests/wakeup_service_path.rs:93,104,122,199 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/carrier_push_storage_tests.rs:47,113,152 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/held_mail_readiness_tests.rs:73,84,101,104 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/interaction_atomic_store_tests.rs:142 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/interaction_history_store.rs:234,259,290 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/interaction_history_store_tests.rs:126,131 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/interaction_notice_pushes_tests.rs:35,63,73,104,121,157,163,202 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/interaction_question_history.rs:32,61 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/mail_delivery_observation_tests.rs:42,209 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/mail_push_retention_tests.rs:14,123 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/message_send_answer_custody_tests.rs:38 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/push_records.rs:43,50 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/src/subscription_progress_epoch_tests.rs:12 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/tests/direct_message_recovery.rs:76,101,334,385 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/tests/push_repository/carrier_outcomes.rs:40,58,85,112,153,169 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/tests/push_repository/reply_references.rs:23,146 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/interaction-storage/tests/push_repository.rs:111,146,340,388,459,502 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/lifecycle-observation/src/lifecycle_store.rs:149,160,216,264,285 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/lifecycle-observation/src/native_observation_stream.rs:270,295 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/lifecycle-observation/tests/address_snapshot.rs:51 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/message-board/src/subscription_membership_tests.rs:14 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/message-board/src/thread_subscriptions_tests.rs:209 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/message-board-storage/src/project_import_batch.rs:32 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/message-board-storage/src/project_request_tests.rs:197 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/tailnet-verification-host/src/bounded_command.rs:41,46 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/tailnet-verification-host/src/host_lifecycle.rs:103,116,132,134,136 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/tailnet-verification-host/src/host_lifecycle_interrupt_tests.rs:28,35 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/tailnet-verification-host/src/host_plan.rs:87 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/tailnet-verification-host/src/owned_host.rs:184,185 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
| crates/workspace-architecture-check/src/workspace_scan.rs:50,137 | context: timestamp or deadline origin; inspect consumer for elapsed-time dependence |
