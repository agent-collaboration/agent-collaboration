# Agent collaboration tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p agent-collaboration --features quota-reset-test-harness
```

- `integration/main.rs` gathers the 61 former CLI, protocol, fixture and opt-in
  live-debug roots. Each root retains its module namespace and ignore policy.
- `integration/conversation_remote_create_cli.rs` exercises remote admission and
  status JSON, frozen request-id retry, first-input files and requester refusals
  through the CLI and an owner-only Router socket with a typed Fleet stand-in.
- `e2e/main.rs` gathers the eight real local Host/central/CLI/MCP journey roots:
  board records, central, takeover, installation directory, messaging rules,
  local mail, reconciliation and retirement. CI prepares the CLI and
  `board-debug-runtime` artifacts, then sets `CODEX_ROUTER_E2E_HOST_BINARY` and
  `CODEX_ROUTER_E2E_PARENT` before running this target serially. These cases
  remain excluded from the main workspace test job.
- Other directories hold case expansions and reusable fixtures. An aggregate
  declares each shared helper implementation once. Colocated `*_test.rs`
  files retain helper unit cases; test-only namespace modules preserve every
  former root's unit-case identity while sharing the helper implementation.

A former binary `<root>` and test `<case>` becomes binary `integration` or
`e2e` and test `<root>::<case>`. Library and binary unit tests keep their names.
Nextest launches each case in its own process. Use `cargo test --no-run` for
compilation measurements; do not replace that process isolation with plain
multi-case `cargo test` execution.
