# Product regression tests

These paths contain permanent product tests or reusable test fixtures. Catalogue/selection tests for removed product code went away; independent ACP history, metadata and required validation regressions remain.


## submitted_prompt_tests.rs

- **Path:** [crates/acp-client-runtime/src/agent_session_client/submitted_prompt_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/acp-client-runtime/src/agent_session_client/submitted_prompt_tests.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Exercises the real ACP actor with a scripted provider to check submitted text, independent native user groups, rejection and sink failures.

- **Why this change remains (source-based explanation):** Retains the regression for lost submitted input independently of provider echo behavior.

## turn_admission_tests.rs

- **Path:** [crates/acp-client-runtime/src/agent_session_client/turn_admission_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/acp-client-runtime/src/agent_session_client/turn_admission_tests.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Checks cancellation and successor-turn isolation; shares the no-interaction fixture and now expects the retained submitted user item.

- **Why this change remains (source-based explanation):** Preserves successor/cancellation assertions while accommodating the new submitted user item.

## claude_filesystem_tests.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_filesystem_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_filesystem_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested Claude skill-file discovery, precedence and malformed-source handling in the removed reader.

## cursor_filesystem_tests.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/cursor_filesystem_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/cursor_filesystem_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested Cursor command/skill file discovery and source errors in the removed reader.

## provider_input_events.rs

- **Path:** [crates/acp-client-runtime/tests/provider_input_events.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/acp-client-runtime/tests/provider_input_events.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Checks that canonical submitted input and provider-emitted user-message groups remain distinct events.

- **Why this change remains (source-based explanation):** Pins the event identities expected by the submitted-input fix.

## owned_host.rs

- **Path:** [crates/agent-collaboration/tests/board_e2e/owned_host.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/agent-collaboration/tests/board_e2e/owned_host.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Starts and stops each test-owned Host and checks that its ready URL matches its own service manifest.

- **Why this change remains (source-based explanation):** Keeps identity-based listener qualification instead of rejecting a valid test-owned port by number.

## board_records_e2e.rs

- **Path:** [crates/agent-collaboration/tests/board_records_e2e.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/agent-collaboration/tests/board_records_e2e.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Runs persisted board journeys and the listener-identity counterexamples, including the opt-in valid-port case.

- **Why this change remains (source-based explanation):** Permanently challenges the listener predicate with accepted/rejected identities and an opt-in real counterexample.

## mail_notice_owner.rs

- **Path:** [crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_owner.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/src/collaboration_runtime/board_composition/tests/mail_notice_owner.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Supplies the test-owned mail store and read-only pending-submission observations for recovery assertions.

- **Why this change remains (source-based explanation):** Makes actual pending submissions inspectable without claiming or executing them.

## notification_authority_recovery.rs

- **Path:** [crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_authority_recovery.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/src/collaboration_runtime/board_composition/tests/notification_authority_recovery.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Checks mail-notification recovery through the owning store and the actual pending outbox submissions.

- **Why this change remains (source-based explanation):** Preserves recovery/replay regressions using actual owning-store evidence.

## provider_native_catalogue_table_tests.rs

- **Path:** [crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue_table_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue_table_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed supervisor catalogue-source outcome combinations.

## provider_native_catalogue_tests.rs

- **Path:** [crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed supervisor route for reading native provider catalogues.

## provider_native_selection_tests.rs

- **Path:** [crates/codex-router-host/src/external_provider_supervisor/provider_native_selection_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/external_provider_supervisor/provider_native_selection_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested rejection and dispatch of selections prepared by the removed catalogue integration.

## claude_plugins_tests.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_plugins_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_plugins_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed Claude plugin export, enablement and detail translation.

## claude_project_roots_tests.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_project_roots_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_project_roots_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested project/worktree root selection for the removed Claude filesystem integration.

## claude_skills_tests.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_skills_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_skills_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested installed and project Claude skill enumeration in the removed integration.

## provider_native_capabilities_tests.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested provider context and source qualification for the removed native catalogue.

## provider_selected_input_tests.rs

- **Path:** [crates/codex-router-host/src/provider_selected_input_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_selected_input_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed translation from a Codex skill/plugin selection to provider input.

## tailscale_exposure/tests.rs

- **Path:** [crates/codex-router-host/src/tailscale_exposure/tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/src/tailscale_exposure/tests.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Tests foreground-lease cleanup using a private Rust child that registers its interrupt handler before advertising readiness.

- **Why this change remains (source-based explanation):** Replaces a racy signal fixture with a private Rust child while retaining own/foreign lease cleanup assertions.

## tailscale_process/tests.rs

- **Path:** [crates/codex-router-host/src/tailscale_process/tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/src/tailscale_process/tests.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Tests normal child shutdown and cancelled observations with a private Rust Serve fixture that handles interrupts deterministically.

- **Why this change remains (source-based explanation):** Replaces the normal signal fixture with a private Rust child while retaining process ownership/shutdown assertions.

## acp_metadata_projection.rs

- **Path:** [crates/codex-router-host/tests/acp_metadata_projection.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-host/tests/acp_metadata_projection.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Exercises actual ACP decoding and Host projection against scripted metadata, checking live and replay text separately.

- **Why this change remains (source-based explanation):** Prevents submitted-input changes from losing metadata or leaking it into assistant prose.

## request_local_websocket_tests.rs

- **Path:** [crates/codex-router-proxy/src/proxy_tests/request_local_websocket_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/codex-router-proxy/src/proxy_tests/request_local_websocket_tests.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Checks WebSocket recovery while observing durable account acquisition and release before shutting down the test runtime.

- **Why this change remains (source-based explanation):** Makes acquisition/release assertions while the write owner is live, preserving the existing recovery outcome.

## router_session_app_server_catalogue_source_tests.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_catalogue_source_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/collaboration-service/src/router_session_app_server_catalogue_source_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed app-server translation of catalogue-source outcomes.

## router_session_app_server_native_capabilities_tests.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_native_capabilities_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/collaboration-service/src/router_session_app_server_native_capabilities_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed app-server native skill-list integration.

## router_session_app_server_plugin_tests.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_plugin_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/collaboration-service/src/router_session_app_server_plugin_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed app-server plugin catalogue/detail projection.

## router_session_app_server_selected_input_tests.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_selected_input_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/collaboration-service/src/router_session_app_server_selected_input_tests.rs)

- **Now:** Removed.

- **Historical purpose:** Tested the removed app-server selected-skill/plugin input translation.

## router_session_app_server_test_support.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_test_support.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-runtime/src/router_session_app_server_test_support.rs)

- **Now:** Moved in base or renumbered; current home: `crates/collaboration-runtime/src/router_session_app_server_test_support.rs`.

- **Disposition:** Moved; identical to #173 at the new home. The earlier branch verification additions do not survive.

- **Purpose now:** Provides shared scripted command/event fixtures for app-server tests; the new runtime home is identical to #173.

## router_session_app_server_tests.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server_tests.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-runtime/src/router_session_app_server_tests.rs)

- **Now:** Moved in base or renumbered; current home: `crates/collaboration-runtime/src/router_session_app_server_tests.rs`.

- **Disposition:** Moved; identical to #173 at the new home. The earlier branch verification additions do not survive.

- **Purpose now:** Tests the ordinary provider app-server protocol; the new runtime home is identical to #173.

## codex_app_server_delivery_route.rs

- **Path:** [crates/collaboration-service/tests/codex_app_server_delivery_route.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-service/tests/codex_app_server_delivery_route.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Tests message delivery through held Codex connections; the surviving change adds receipt/binding/backend diagnostics when the existing assertion fails.

- **Why this change remains (source-based explanation):** Improves diagnosis of the existing held-connection assertion; no extra passing condition or product mechanism.

## codex_app_server_held_thread_route.rs

- **Path:** [crates/collaboration-service/tests/codex_app_server_held_thread_route.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-service/tests/codex_app_server_held_thread_route.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Tests held-thread message delivery and permissions; the surviving change adds the same failure diagnostics without changing the pass condition.

- **Why this change remains (source-based explanation):** Improves diagnosis of the existing held-thread assertion; no extra passing condition or product mechanism.

## public_observation_recovery.rs

- **Path:** [crates/collaboration-service/tests/public_observation_recovery.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-service/tests/public_observation_recovery.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Tests public lifecycle recovery; the surviving change retains progress and task-state diagnostics when the original readiness deadline fails.

- **Why this change remains (source-based explanation):** Keeps the original readiness deadline and reports where the fixture/observer stopped on failure.

## acp_provider_backend.rs

- **Path:** [crates/collaboration-service/tests/support/acp_provider_backend.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-service/tests/support/acp_provider_backend.rs)

- **Now:** Present, unchanged from #173.

- **Purpose now:** Provides a scripted ACP provider backend for service tests; this branch leaves the base file unchanged.
