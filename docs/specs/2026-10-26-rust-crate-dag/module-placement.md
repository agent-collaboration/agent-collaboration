# Module placement (frozen 2026-10-26): every top-level module of today's collaboration-service

Sources:
- the independent design review (`tmp/2026-10-26-design-review-dag.md`), whose corrected table this is;
- the Advisor (`tmp/2026-10-26-advisor-dag.md`);
- the Lead's own check: running this table through the measured module graph (cargo-modules' resolved `use` edges plus explicit `crate::<module>::` body paths) leaves 3 upward edges, all handled below.

Split rows:
- `service_identity` dissolves: `RouterIdentity` values go to contracts; per-family handles go to the application; startup wiring goes to the Host.
- `project_replication_position`: the port and default go to contracts; the `NativeProjectStore` cursor adapter goes to runtime.
- `wakeup_projection`: snapshots and mutation projection stay in the application; the `timestamp` conversion moves to runtime.

| Module | Crate |
|---|---|
| `machine_identity` | collaboration-contracts |
| `mail_fetch_budget_tests` | collaboration-contracts |
| `native_delivery_result` | collaboration-contracts |
| `native_route_claim` | collaboration-contracts |
| `project_claim` | collaboration-contracts |
| `project_registration` | collaboration-contracts |
| `project_replication_position` | collaboration-contracts |
| `provider_conversation_backend` | collaboration-contracts |
| `router_delivery_types` | collaboration-contracts |
| `scheduled_run_contract` | collaboration-contracts |
| `session_command_port` | collaboration-contracts |
| `session_delivery_contract` | collaboration-contracts |
| `session_event_hub` | collaboration-contracts |
| `acp_channel_listener` | collaboration-runtime |
| `app_server_event_forwarding` | collaboration-runtime |
| `app_server_interaction_translation_tests` | collaboration-runtime |
| `app_server_item_metadata` | collaboration-runtime |
| `app_server_item_translation` | collaboration-runtime |
| `app_server_item_translation_tests` | collaboration-runtime |
| `app_server_model_catalog` | collaboration-runtime |
| `automation_configuration` | collaboration-runtime |
| `automation_retention_worker` | collaboration-runtime |
| `central_project_reads` | collaboration-runtime |
| `codex_app_server_delivery_route` | collaboration-runtime |
| `codex_app_server_scheduled_runs` | collaboration-runtime |
| `codex_conversation_operation_recorder` | collaboration-runtime |
| `codex_queue_reconciliation` | collaboration-runtime |
| `delivery_acceptance_effect` | collaboration-runtime |
| `delivery_reconciliation` | collaboration-runtime |
| `delivery_route_projection` | collaboration-runtime |
| `endpoint_directory` | collaboration-runtime |
| `fleet_client` | collaboration-runtime |
| `interaction_broker` | collaboration-runtime |
| `interaction_storage_failure` | collaboration-runtime |
| `mail_delivery_projection` | collaboration-runtime |
| `mail_delivery_session` | collaboration-runtime |
| `mail_push_preparation` | collaboration-runtime |
| `manifest_publication` | collaboration-runtime |
| `message_effect_state` | collaboration-runtime |
| `native_channel_relay` | collaboration-runtime |
| `native_control_backend` | collaboration-runtime |
| `native_generation_gate` | collaboration-runtime |
| `native_message_dispatch` | collaboration-runtime |
| `native_recorded_permission_tests` | collaboration-runtime |
| `native_recorded_policy` | collaboration-runtime |
| `native_relay_listener` | collaboration-runtime |
| `native_thread_preparation` | collaboration-runtime |
| `outgoing_mail_message` | collaboration-runtime |
| `pending_snapshot_interactions` | collaboration-runtime |
| `private_socket_listener` | collaboration-runtime |
| `project_claim_store` | collaboration-runtime |
| `project_read_handle` | collaboration-runtime |
| `project_store_paths` | collaboration-runtime |
| `project_store_registry` | collaboration-runtime |
| `provider_acp_content_translation` | collaboration-runtime |
| `provider_acp_event_projection` | collaboration-runtime |
| `provider_acp_interaction` | collaboration-runtime |
| `provider_acp_session_route` | collaboration-runtime |
| `provider_operation_store` | collaboration-runtime |
| `provider_session_event_hub` | collaboration-runtime |
| `provider_session_record` | collaboration-runtime |
| `push_handoff_worker` | collaboration-runtime |
| `received_mail_push` | collaboration-runtime |
| `router_session_app_server` | collaboration-runtime |
| `run_reconciliation` | collaboration-runtime |
| `schedule_preparation_evidence_sink` | collaboration-runtime |
| `schedule_timing_worker` | collaboration-runtime |
| `scheduled_native_dispatch` | collaboration-runtime |
| `scheduled_native_observation` | collaboration-runtime |
| `scheduled_run_evidence_sink` | collaboration-runtime |
| `scheduled_run_router` | collaboration-runtime |
| `scheduled_run_worker` | collaboration-runtime |
| `service_identity_storage` | collaboration-runtime |
| `session_delivery_router` | collaboration-runtime |
| `session_display_name_cache` | collaboration-runtime |
| `stored_delivery_receipt` | collaboration-runtime |
| `stored_inventory_observation` | collaboration-runtime |
| `stored_run_receipt` | collaboration-runtime |
| `summary_native_worker` | collaboration-runtime |
| `unmaterialized_thread_holder` | collaboration-runtime |
| `wakeup_delivery_sender` | collaboration-runtime |
| `wakeup_timing_worker` | collaboration-runtime |
| `attempt_history_projection` | collaboration-service (application) |
| `automation_collection_cursor` | collaboration-service (application) |
| `automation_collection_projection` | collaboration-service (application) |
| `automation_event_cursor` | collaboration-service (application) |
| `automation_event_projection` | collaboration-service (application) |
| `automation_inspection_failure` | collaboration-service (application) |
| `board_views` | collaboration-service (application) |
| `codex_session_inventory` | collaboration-service (application) |
| `collaboration_application` | collaboration-service (application) |
| `conversation_operation_projection` | collaboration-service (application) |
| `delivery_projection` | collaboration-service (application) |
| `frozen_mail_query_fixture` | collaboration-service (application) |
| `instruction_projection` | collaboration-service (application) |
| `mail_delivery_runtime` | collaboration-service (application) |
| `mail_push_delivery` | collaboration-service (application) |
| `mail_push_outcome` | collaboration-service (application) |
| `native_claude_catalogue` | collaboration-service (application) |
| `notification_publisher` | collaboration-service (application) |
| `operation_receipt_projection` | collaboration-service (application) |
| `project_admission` | collaboration-service (application) |
| `project_commit_publication` | collaboration-service (application) |
| `provider_session_inventory` | collaboration-service (application) |
| `push_record_delivery` | collaboration-service (application) |
| `push_record_resolver` | collaboration-service (application) |
| `run_projection` | collaboration-service (application) |
| `schedule_activation` | collaboration-service (application) |
| `schedule_preparation` | collaboration-service (application) |
| `schedule_projection` | collaboration-service (application) |
| `service_identity` | collaboration-service (application) |
| `test_project_runtime` | collaboration-service (application) |
| `test_subject_workstream` | collaboration-service (application) |
| `wake_creation_crash_tests` | collaboration-service (application) |
| `wakeup_projection` | collaboration-service (application) |
| `wakeup_subscription` | collaboration-service (application) |
| `native_worker_faults` | test support beside consumers |

## The three residual upward edges and their treatment

1. `delivery_route_projection → delivery_projection`: test-only (`delivery_route_projection.rs:65,93,132`). Move those tests to application or integration coverage.
2. `native_control_backend → collaboration_application`: `#[cfg(test)]` only (`native_control_backend.rs:6,23,54`). Move those tests beside the session application tests.
3. `scheduled_native_dispatch → wakeup_projection::timestamp` (`scheduled_native_dispatch.rs:275`). Extract that one conversion into a runtime module named for it; the wake serializer stays in the application.
