# Existing default owner

Reference: [Specification](specification.md), R1–R4.

The existing CLI Host owns selection. Change only the normal branch of `crates/codex-router-cli/src/host_command/mod.rs::default_mcp_bind` to 19742. `HostCommand::effective_mcp_bind` factors the existing optional-bind resolution inside that owner; `run_host_command` uses it when constructing `ForegroundHostInputs`. This permits a permanent parsed-command test to exercise the actual resolution rather than duplicating its formula.

Production proxy selection still consumes `codex-native-integration::DEFAULT_ROUTER_PORT` (19741). Debug defaults and explicit arguments keep their existing owners. `RouterConfig::for_test` using 8787 is an intentional fixture, not a production default.

Update the existing CLI default oracle and active `docs/agent-guidance/agent-collaboration.md`. Preserve captured replacement arguments, normal debug guidance and historical evidence. Workspace package/dependency declarations and Cargo.lock advance together to the next unused version; 0.1.97 was unused when selected and must be rechecked before publication.

No new module, public API, wire shape, persisted state, dependency, trust control or failure policy is required. The separate `codex/tailnet-activation` worktree starts at main `ffced1c1`; it does not import in-progress UI changes or the original dirty Tailnet snapshot. Private staged configuration remains external to the source repository.
