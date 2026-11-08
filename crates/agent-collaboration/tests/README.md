# Agent collaboration tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p agent-collaboration --features quota-reset-test-harness
```

- `integration/main.rs` gathers the 61 former CLI, protocol, fixture and opt-in
  live-debug roots. Each root retains its module namespace and ignore policy.
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

## Test folder ownership

- `integration/automation_live_support`: integration modules, moved from `automation_live_support/`.
- `e2e/board_e2e`: e2e modules, moved from `board_e2e/`.
- `integration/board_live_support`: integration modules, moved from `board_live_support/`.
- `integration/board_skill_dx_support`: integration modules, moved from `board_skill_dx_support/`.
- `integration/claude_background_cli`: integration modules, moved from `claude_background_cli/`.
- `integration/conversation_fault_entry_paths`: integration modules, moved from `conversation_fault_entry_paths/`.
- `integration/delivery_matrix`: integration modules, moved from `delivery_matrix/`.
- `e2e/mail_reconcile`: e2e modules, moved from `mail_reconcile/`.
- `e2e/mail_retirement`: e2e modules, moved from `mail_retirement/`.
- `integration/messaging_skill_dx_support`: integration modules, moved from `messaging_skill_dx_support/`.
- `integration/provider_conversation_cli`: integration modules, moved from `provider_conversation_cli/`.
- `integration/thread_subscription_cli`: integration modules, moved from `thread_subscription_cli/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
