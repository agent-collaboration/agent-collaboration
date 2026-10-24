# What must be true

Discovery tells you what is available. Selection asks the native provider to use a particular capability. Conversation display must keep metadata separate from the assistant’s words.

Read the exact skill, plugin, command and metadata obligations below when checking an implementation. Unsupported selections fail explicitly; they are not silently dropped.

Only Router/Host adaptation is in scope. Codex, providers, ACP adapters and upstream ACP remain unchanged. A feature their existing contracts cannot supply is reported as unsupported; no replacement runtime or new protocol is introduced.

## Who interacts with the integration

```mermaid
flowchart TD
  User[Developer using Codex] -->|discover and select| System[Provider capability integration]
  System -->|catalogue and selection outcomes| User
  Provider[Selected native provider] -->|native state and execution outcomes| System
  System -->|native discovery and invocation requests| Provider
  Operator[Owner operating debug target] -->|declared isolated target| System
  System -->|bounded proof observations| Operator
  Protected[Production Router and other lanes]
```

The integration is opaque in this view. Production and unrelated lanes have no mutation edge.
