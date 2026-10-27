# Embedded reader tests

These were tests embedded inside provider-file interpretation modules. Their source readers were removed with the rejected native catalogue.


## claude_legacy_commands.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_legacy_commands.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_legacy_commands.rs)

- **Now:** Removed.

- **Historical purpose:** Converts Claude 2.1.310 legacy command Markdown files into skill catalogue entries, with canonical names, bounded scanning, and explicit candidate errors; it does not execute commands.

- **Original reason (reconstructed from code):** Legacy files need a separate provider-specific interpretation path while retaining their native command names and directory structure.

## claude_plugin_export.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_plugin_export.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_plugin_export.rs)

- **Now:** Removed.

- **Historical purpose:** Decodes Claude 2.1.310's `plugin list --json` export and validates the regular installed-plugin key format and scope.

- **Original reason (reconstructed from code):** The plugin list is a version-specific native export, distinct from skill files and consumer-facing plugin identifiers.

## claude_plugin_skills.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_plugin_skills.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_plugin_skills.rs)

- **Now:** Removed.

- **Historical purpose:** Reads skill roots for installed Claude plugins after setup has qualified plugin name, root order, enablement, and wrapper conditions.

- **Original reason (reconstructed from code):** Plugin skill roots have different native names and qualification inputs from ordinary Claude skills.

## claude_skill_frontmatter.rs

- **Path:** [crates/acp-client-runtime/src/native_capabilities/claude_skill_frontmatter.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/acp-client-runtime/src/native_capabilities/claude_skill_frontmatter.rs)

- **Now:** Removed.

- **Historical purpose:** Parses supported Claude skill frontmatter into declared metadata facts and errors on unsupported or invalid YAML and invocation flags; it does not infer enablement or native loading from absence.

- **Original reason (reconstructed from code):** Separating parsed declarations from effective setup state prevents file contents from being mistaken for provider enablement.

## claude_project_context.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/claude_project_context.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/claude_project_context.rs)

- **Now:** Removed.

- **Historical purpose:** Qualifies Claude project/worktree context and locally configured skill roots from the pinned native setup sources.

- **Original reason (reconstructed from code):** Workspace-level skill visibility depends on native project context and settings, not only the path of an individual skill file.

## cursor_executable.rs

- **Path:** [crates/codex-router-host/src/provider_native_capabilities/cursor_executable.rs](https://github.com/agent-collaboration/agent-collaboration/blob/5f5c49a4e29bc9a542965d4006f922038dc816a7/crates/codex-router-host/src/provider_native_capabilities/cursor_executable.rs)

- **Now:** Removed.

- **Historical purpose:** Captures and revalidates identity for the exact Cursor executable used by the provider launch, including qualified versioned launcher resolution.

- **Original reason (reconstructed from code):** Cursor ACP does not advertise a version in its initialize capabilities, so the Host checks the launch's executable source instead of searching for a global Cursor command.
