# What each provider actually exposes

**Six providers were checked. Their ACP support is uneven.** The findings below describe source at identified revisions; they are not installed-provider or terminal-menu proof. Every provider still owns its own setup.

## Codex: usable convention and native skill API

The open-source `codex-acp` bridge already asks Codex app-server for `skills/list`. That native result has names, paths, scope and plugin identity. The bridge advertises each skill as **`$name`** in its ACP command list, alongside its built-in slash commands. A Codex-specific shim can recognize that convention without Router scanning skill files. Updates are driven by app-server skill invalidations.

The ACP rows normally drop the richer origin fields; AIR alone receives a skill path. Selection is name-based in this bridge, so do not assume path-level identity or collision-free dispatch. Codex acting as an ACP provider is distinct from Codex acting as our terminal client.

Source: [`CodexCommands.ts`, revision 0539d3da](https://github.com/agentclientprotocol/codex-acp/blob/0539d3da63948f55be3a90727fc550db40c5e8bb/src/CodexCommands.ts#L155-L219). [Detailed source map](../../../tmp/2026-10-26-menu-design-research/codex-acp-provider-apis.md).

## Claude: an existing extension, with limits

Claude ACP 0.81.2 has an opt-in **`_claude/sdkMessage`** notification. Filtering it to SDK init and command-change messages can expose skill names separately from command information. This is an existing vendor extension worth evaluating before proposing a new method. Its message filter selects whole messages, not individual fields: init includes more than menu data. Any approved consumer should extract only needed metadata before recording it.

This is not yet a proven complete classifier: aliases, duplicate names, the relationship to filtered ACP commands and later updates still need qualification. Upstream 0.97.0 also adds AIR-specific skill annotations, but derives them by searching skill files. We should not copy that scanner into Router.

Sources: [0.81.2 extension option](https://github.com/agentclientprotocol/claude-agent-acp/blob/5dbb453c63a89746627799b2b06b31ba01a1b674/src/acp-agent.ts#L1345-L1384), [0.97.0 AIR projection](https://github.com/agentclientprotocol/claude-agent-acp/blob/d52c7c64d8e4b3e5bb988658fed4cd487b9f935c/src/acp-agent.ts#L13094-L13185). [Detailed comparison](../../../tmp/2026-10-26-menu-design-research/claude-existing-extensions.md).

## Cursor: skill identity is lost at the ACP boundary

In Cursor CLI **2026.10.22-27dcf5f**, the loader distinguishes custom commands and skills internally. Its ACP sender combines them and emits only name and description. The inspected inbound extension handler supports model listing; it does not expose a skill-list route.

A client shim can consume the command list, but cannot exactly separate skills from other commands from those rows alone. **Owner-selected interim behavior:** show Cursor's advertised rows under `/`, with classification marked unknown. Heuristics are permitted for likely skills; do not claim an inferred category came from Cursor. The initial source-backed fallback needs no copied loader or Cursor vendor change.

Source: [exact installed-bundle hashes, offsets and handler reads](../../../tmp/2026-10-26-menu-design-research/cursor-existing-extensions.md). Cursor’s implementation is proprietary; the report does not substitute a community client for it.

## Pi: structured native data, flattened by its ACP bridge

Pi’s native RPC **`get_commands`** returns `source: skill | prompt | extension`. Skills use the invocation name **`skill:name`**. The community `pi-acp` 0.0.34 bridge preserves that prefix in standard ACP rows, so a Pi-specific shim can classify those advertised skills and submit `/skill:name` through the normal prompt path.

The bridge filters extension commands by default and publishes the list at new/load, without a live catalogue-change subscription. Therefore it does not establish the full `/` list you requested. The retained `skill:` prefix is a useful provider-specific classification rule for the shim, within your permission to use pragmatic heuristics. Pi’s native RPC/SDK is richer, but using it directly is a separate integration decision.

Sources: [Pi native RPC, revision 6c384aa3](https://github.com/earendil-works/pi/blob/6c384aa30f619d7b28963f2bab24d8dacbe73cb6/packages/coding-agent/src/modes/rpc/rpc-mode.ts#L676-L711), [released bridge converter](https://github.com/svkozak/pi-acp/blob/b0581c9c1d675e634234674484247008b03d69b4/src/acp/pi-commands.ts#L22-L58). [Detailed comparison](../../../tmp/2026-10-26-menu-design-research/pi-provider-apis.md).

## OpenCode: commands over ACP, skills through a separate native API

OpenCode **2.1.4** advertises its registered commands and built-in `compact` using standard ACP command updates. Command changes trigger catalogue refresh. That catalogue does not include a skill-kind field or a separate skill list.

Its native HTTP API has `GET /api/skill` and an experimental session skill-activation route. Those are useful candidates for an OpenCode shim if a native-API exception is approved; they are not ACP methods. Skill selection uses a skill ID, and the native UI carries it separately from ordinary command text. Sending an invented `/skill` command would not reproduce that contract.

Sources: [ACP command projection at 0b050ac5](https://github.com/anomalyco/opencode/blob/0b050ac5cdd995ba8165e0699d4150b3409cfe51/packages/cli/src/acp/config-option.ts#L70-L75), [native skill list](https://github.com/anomalyco/opencode/blob/0b050ac5cdd995ba8165e0699d4150b3409cfe51/packages/protocol/src/groups/skill.ts#L7-L20), [native activation](https://github.com/anomalyco/opencode/blob/0b050ac5cdd995ba8165e0699d4150b3409cfe51/packages/protocol/src/groups/session.ts#L378-L395). [Detailed source map](../../../tmp/2026-10-26-menu-design-research/opencode-provider-apis.md). OpenCode 1.x was not qualified in this pass.

## Gemini: native UI skills are not in this ACP catalogue

The inspected Gemini CLI ACP handler has a separate fixed registry of **six commands**. Its interactive command loaders can create skill actions, but those loaders are not wired into this ACP catalogue.

A shim can show the six advertised ACP commands. It cannot promise the interactive skills or all other terminal commands without a provider-side change or another approved integration.

Sources: [ACP handler at 363a1311](https://github.com/google-gemini/gemini-cli/blob/363a13119a57354f469732577518b9d6b319d1d6/packages/cli/src/acp/acpCommandHandler.ts#L23-L38), [separate skill loader](https://github.com/google-gemini/gemini-cli/blob/363a13119a57354f469732577518b9d6b319d1d6/packages/cli/src/services/SkillCommandLoader.ts#L24-L55). [Detailed source map](../../../tmp/2026-10-26-menu-design-research/gemini-provider-apis.md).

## What “all commands” must account for

An agent’s terminal may have controls that require its own interactive UI. ACP providers often omit those controls or implement only a headless subset. Showing a command name does not prove it can run through ACP. The common interface should describe commands the connected provider exposes and can execute; covering additional terminal-only controls requires an explicit decision rather than quietly treating them as model prose.
