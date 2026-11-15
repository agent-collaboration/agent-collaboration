# ACP provider adapter tests

`integration/main.rs` groups the provider launch contracts, the real TypeScript
metadata producer decoded by the Rust adapter, and the auto permission-mode plan
(mode, scratch-folder grant, in-folder allowance) driven through the ACP runtime
against `verification/fixtures/permission-mode-agent.ts`. Unit tests remain beside their
implementation.

Run `cargo nextest run --locked -p acp-provider-adapters`.
