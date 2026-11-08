# ACP client runtime tests

Run with the feature set enabled in workspace CI:

```sh
cargo nextest run --locked --profile ci -p acp-client-runtime --features test-observation
```

`integration/main.rs` gathers the external roots, including the bridge behavior cases. Cases cover provider
lifecycle and transport, prompt boundaries and output, settings and history
ordering, permissions, questions, cancellation, input and idle events. Library
unit tests remain colocated.

Case expansions live under `integration/`; fixture data retain their existing directories. The aggregate declares the shared provider-behavior helper once. CI selects no separate e2e category,
so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

## Test folder ownership

- `integration/create_settings_order`: integration modules, moved from `create_settings_order/`.
- `integration/cursor_model_settings`: integration modules, moved from `cursor_model_settings/`.
- `integration/provider_turn_boundaries`: integration modules, moved from `provider_turn_boundaries/`.

Existing `support/` and fixture data stay in place. Module names, case identities
and process selectors are unchanged.
