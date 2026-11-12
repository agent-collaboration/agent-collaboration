# Agent collaboration tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p agent-collaboration --features quota-reset-test-harness
```

- `integration/main.rs` gathers the 23 former CLI, protocol and fixture roots
  for the kept surfaces: sessions and conversations, approvals and questions,
  provider operations, endpoints, whoami, the journal and the native and ACP
  stdio bridges. Each root retains its module namespace and ignore policy.
- Other directories hold case expansions and reusable fixtures. An aggregate
  declares each shared helper implementation once. Colocated `*_test.rs`
  files retain helper unit cases.

The board, message, mail, wake, schedule, automation and central-admin
commands were removed, and their integration and e2e roots went with them.
No e2e category remains, so no e2e target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library and binary unit tests keep their names.
Nextest launches each case in its own process. Use `cargo test --no-run` for
compilation measurements; do not replace that process isolation with plain
multi-case `cargo test` execution.
