# Test/module wiring

These source files mounted or supported the tested behavior. A surviving file may contain ordinary ACP behavior after the catalogue-specific additions were removed; its presence alone does not mean the old integration remains.


## agent_session_client.rs

- **Path:** [crates/acp-client-runtime/src/agent_session_client.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/acp-client-runtime/src/agent_session_client.rs)

- **Now:** Changed in surviving branch.

- **Purpose now:** Owns the ACP client/session operations; the surviving addition mounts the submitted-input regression module.

- **Why this change remains (source-based explanation):** Makes the permanent submitted-input regressions part of the runtime test target.

## claude_filesystem.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_filesystem.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_filesystem.rs)

- **Now:** Removed.

- **Historical purpose:** Implemented filesystem traversal and metadata interpretation for the removed Claude skill reader.

## cursor_filesystem.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/cursor_filesystem.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/cursor_filesystem.rs)

- **Now:** Removed.

- **Historical purpose:** Implemented filesystem traversal and metadata interpretation for the removed Cursor command/skill reader.

## provider_native_catalogue.rs

- **Path:** [crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/external_provider_supervisor/provider_native_catalogue.rs)

- **Now:** Removed.

- **Historical purpose:** Connected supervisor requests to the removed native catalogue and selection helpers.

## provider_native_capabilities.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities.rs)

- **Now:** Removed.

- **Historical purpose:** Assembled provider-specific filesystem/export catalogue reads in Host; this integration was removed.

## claude_plugins.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_plugins.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_plugins.rs)

- **Now:** Removed.

- **Historical purpose:** Translated installed Claude plugin exports and plugin-owned skill roots for the removed integration.

## claude_skills.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_skills.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_skills.rs)

- **Now:** Removed.

- **Historical purpose:** Combined ordinary and plugin Claude skill sources for the removed integration.

## provider_selected_input.rs

- **Path:** [crates/codex-router-host/src/provider_selected_input.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_selected_input.rs)

- **Now:** Removed.

- **Historical purpose:** Resolved and validated catalogue-backed skill/plugin selections before provider dispatch; this path was removed.

## router_session_app_server.rs

- **Path:** [crates/collaboration-service/src/router_session_app_server.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-runtime/src/router_session_app_server.rs)

- **Now:** Moved in base or renumbered; current home: `crates/collaboration-runtime/src/router_session_app_server.rs`.

- **Disposition:** Moved; identical to #173 at the new home. The earlier branch verification additions do not survive.

- **Purpose now:** Implements the provider app-server front door in its new runtime home; no branch additions survive there.

## session_command_port.rs

- **Path:** [crates/collaboration-service/src/session_command_port.rs](https://github.com/agent-collaboration/agent-collaboration/blob/424fbc238/crates/collaboration-contracts/src/session_command_port.rs)

- **Now:** Moved in base or renumbered; current home: `crates/collaboration-contracts/src/session_command_port.rs`.

- **Disposition:** Moved; identical to #173 at the new home. The earlier branch verification additions do not survive.

- **Purpose now:** Defines provider-neutral session command traits in the contracts crate; no branch additions survive there.
