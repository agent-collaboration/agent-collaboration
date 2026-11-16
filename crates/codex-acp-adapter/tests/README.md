# Codex ACP adapter tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-acp-adapter
```

`integration/main.rs` gathers the 19 former external roots. Cases cover ACP
connections, frames and request routing; native session creation and stored
history; prompt execution, cancellation and settlement; and permission, content,
progress and MCP projections. Library unit tests remain colocated.

`acp_served_n8_node_tap` is N8's ACP wire tap: it drives `serve_acp_connection` over
a Unix socket pair and records every ACP line in both directions as it crossed, plus
the fake Codex app-server's socket transcript. The unit N8 taps record
`PendingAcpPrompt::next_event` results as typed outcomes, not as wire frames.

The aggregate declares shared permission-echo and conversation-recorder helpers
once. Case expansions and fixture data retain their existing directories. No
empty e2e target is declared because CI selects no separate e2e category here.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
