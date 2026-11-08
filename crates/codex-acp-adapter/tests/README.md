# Codex ACP adapter tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-acp-adapter
```

`integration/main.rs` gathers the 19 former external roots. Cases cover ACP
connections, frames and request routing; native session creation and stored
history; prompt execution, cancellation and settlement; and permission, content,
progress and MCP projections. Library unit tests remain colocated.

The aggregate declares shared permission-echo and conversation-recorder helpers
once. Case expansions and fixture data retain their existing directories. No
empty e2e target is declared because CI selects no separate e2e category here.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

## Test folder ownership

- `integration/native_prompt_execution`: integration modules, moved from `native_prompt_execution/`.
- `integration/session_creation`: integration modules, moved from `session_creation/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
