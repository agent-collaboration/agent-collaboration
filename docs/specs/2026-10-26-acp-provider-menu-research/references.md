# What ACP, Orca and T3 Code contribute

## ACP gives us the transport, not a universal skills catalogue

The current ACP repository explicitly identifies **wire protocol 1 as stable**. Router currently pins Rust runtime **2.2.0** and schema crate **1.9.1**; those package versions are distinct from the negotiated wire version. The newer v2 material is not evidence that this Router/provider path uses v2. [ACP versioning](https://github.com/agentclientprotocol/agent-client-protocol/blob/69344e538d5f1f4b9c2dd28572cfaffcc37e6988/README.md#L19-L29).

Standard v1 supplies:

- `available_commands_update`: the session’s advertised command rows, with names, descriptions and optional input hints.
- `session/prompt`: the normal way to invoke a command as text, with other prompt content allowed.
- `_meta` and underscore-prefixed extension methods/notifications, with capabilities used to advertise custom support.

It does **not** standardize a separate skill list or a skill-versus-command discriminator. The v2 command documentation inspected also has only the `command` type; its presence does not establish a standard skill kind. [v1 commands](https://github.com/agentclientprotocol/agent-client-protocol/blob/69344e538d5f1f4b9c2dd28572cfaffcc37e6988/docs/protocol/v1/slash-commands.mdx), [v2 command type](https://github.com/agentclientprotocol/agent-client-protocol/blob/69344e538d5f1f4b9c2dd28572cfaffcc37e6988/docs/protocol/v2/slash-commands.mdx#L89-L109).

The official TS/Rust libraries provide protocol types and custom-message handling. They make a selective extension possible; they do not make every provider implement a skill catalogue. No new menu framework or SDK upgrade follows merely from this research. [Extension rules](https://github.com/agentclientprotocol/agent-client-protocol/blob/69344e538d5f1f4b9c2dd28572cfaffcc37e6988/docs/protocol/v1/extensibility.mdx), [TS typed custom handlers](https://github.com/agentclientprotocol/typescript-sdk/blob/bca8e49199927e167fab74db4dd76dec4743bb49/src/v1/acp.ts#L2126-L2209).

## Orca: borrow its shared menu and construction boundary

**Updated after your review:** the latest Orca and T3 source pins are checked in [What to reuse and why inject the shims](decoupling.html). That page maps concrete source patterns to the separate Rust crate and Host injection boundary.

Orca has one session-command row with `kind: command | skill`, a command reader on its common session adapter, and a shared composer that splits the rows. That is close to the small interface we need. Native Codex obtains skills through app-server; Pi obtains typed command rows through native RPC; Claude interprets SDK catalogue messages. [Row contract](https://github.com/stablyai/orca/blob/4e0f391c382f0189e70b57e8fc2848f02d39d157/src/shared/agent-session-wire.ts#L412-L429), [shared menu](https://github.com/stablyai/orca/blob/4e0f391c382f0189e70b57e8fc2848f02d39d157/src/shared/native-chat-slash-commands.ts#L135-L170).

But Orca is not proof that generic ACP already distinguishes skills. Its generic ACP converter labels every advertised row as a command. It also retains curated terminal catalogues and filesystem skill discovery for other paths. The scanner and curated-list choices exceed our boundary. Treating generic rows as commands is now an allowed interim slash fallback, provided their unknown classification is retained. [ACP conversion](https://github.com/stablyai/orca/blob/4e0f391c382f0189e70b57e8fc2848f02d39d157/src/main/acp/acp-structured-options.ts#L105-L121), [composer sources](https://github.com/stablyai/orca/blob/4e0f391c382f0189e70b57e8fc2848f02d39d157/src/renderer/src/components/native-chat/use-native-chat-composer-catalog.ts#L25-L58).

**Borrow:** typed construction, common rows and shared rendering. **Keep explicit:** unknown kind, inferred kind and native invocation name. Keep filesystem scanners and curated terminal catalogues out.

## T3 Code v2: keep menu data separate from orchestration

T3’s common V2 adapter owns running sessions, turns and runtime capabilities. Menu data instead travels through provider/workspace snapshots containing skills and slash commands. The composer reads the selected provider’s snapshot; ACP command changes feed that provider state. This is a useful separation: menu discovery does not require a new orchestration engine. [Workspace snapshot](https://github.com/pingdotgg/t3code/blob/a2398c2ce33c4b964b4d9814644ac6b83521da60/packages/contracts/src/server.ts#L96-L132), [adapter contract](https://github.com/pingdotgg/t3code/blob/a2398c2ce33c4b964b4d9814644ac6b83521da60/packages/provider-core/src/server/ProviderAdapter.ts#L543-L647).

Its ACP registry shim recognizes `$`-prefixed command names as skills and creates a synthetic skill URI. That is its own convention, not a universal ACP rule. Other native provider paths use richer integrations. We should preserve the useful separation without importing its caches, synthetic path scheme or broader orchestration machinery. [ACP normalization](https://github.com/pingdotgg/t3code/blob/a2398c2ce33c4b964b4d9814644ac6b83521da60/packages/provider-acp-registry/src/server/probe.ts#L212-L247).

**Borrow:** one client menu model fed by provider-specific translation. **Keep out:** runtime scheduling, snapshots and storage mechanisms that our menu requirements do not need. [Detailed T3 source map](../../../tmp/2026-10-26-menu-design-research/t3-provider-interface.md).

## What was verified

The public repositories were cloned into `~/Documents/dev/open-source/`; decisive source paths were opened after DeepWiki supplied navigation leads. Cursor was checked against the identified installed code bundle because its ACP implementation is proprietary. No provider session, model call, installation or production process change was made for this research.

The HTML is prepared from these Markdown sources. Static checks and source inspection are separate from browser readability and actual terminal behavior. The next proof must exercise the agreed menu design on the selected real client/provider versions; this document does not claim that proof.
