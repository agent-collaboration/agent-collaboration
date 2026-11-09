# Fleet copy workerd fixture

This existing upstream adapter fixture moved from agent-fleet-sync b9470dc. It
calls production fleet-project-copy rules through the real SQLx Durable Object
adapter. It is a standalone, unpublished wasm workspace with its own lockfile
and target directory. Production packages must never depend on it.

The TypeScript controllers in `verification/fixtures/fleet-copy-workerd` retain
five native-writer rename streams, four copy-certainty controls, 22 foreign-key
controls, and optional snapshot replay. Seven separate raw-SQL migration cases
live in `verification/fixtures/fleet-migrations-workerd`. These are local
workerd checks, not hosted Cloudflare acceptance.

Build with the repository toolchain and wasm-bindgen CLI 0.2.129. The launcher
uses the existing verification package's pinned Miniflare/workerd installation
and compatibility date 2026-07-29. A missing exact case, empty result, failed
case or prefix-only stream fails the run. Certainty faults fabricate a lost
boundary reply after a real commit/rollback; they are not physical I/O faults.

The Advisor cleared relocating this crate before its first build. Its direct
closure is the production copy/protocol rules and general DO/SQLx/runtime
adapters, workers-rs and Serde. It does not pull the native service or Central
composition into wasm. Whole combined CI cold/warm measurements remain required.
