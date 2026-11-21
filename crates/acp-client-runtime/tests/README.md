# ACP client runtime tests

Run with the feature set enabled in workspace CI:

```sh
cargo nextest run --locked --profile ci -p acp-client-runtime --features test-observation
```

`integration/main.rs` gathers the external roots, including the bridge behavior cases. Cases cover provider
lifecycle and transport, prompt boundaries and output, settings and history
ordering, permissions, questions, cancellation, input and idle events. Library
unit tests remain colocated.

`integration/simulated_races.rs` holds the client-level race scenarios on Turmoil's
deterministic runtime (design note §11.8): the real client over a duplex pair to
the in-process `simulated_races/scripted_line_agent.rs`. They use the crate's
`test-support` feature (the in-process transport and the named-point hook, which
they forward to Turmoil's barriers); Turmoil is a dev-dependency only.

The ACP adapter suite shares the owner fixture through this crate's non-default
`test-support` feature. Both test targets drive the same owner and wire paths.

Case expansions and fixture data retain their existing directories. The aggregate declares the shared provider-behavior helper once, and
`acp_wire_capture.rs`, which records a fixture agent's complete stdio capture (written by
`verification/fixtures/acp-stdio.ts` when given `wireCapture`) as raw wire frames on N6 tap cases;
`integration/typed_extension_node_tap/` holds that tap's fixture rig. CI selects no separate e2e category,
so only the integration target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
