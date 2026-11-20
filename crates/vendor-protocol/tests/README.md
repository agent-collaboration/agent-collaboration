# Vendor protocol tests

The ACP settings codec's colocated unit tests cover omitted and empty option
sets, grouped offers in provider order, and current select and Boolean values.
The runtime's scripted ACP integration tests exercise the catalog consumer.

The integration target exercises the public Codex `model/list`, `thread/read`,
and `thread/resume` bindings with independent JSON samples and compiler
diagnostics for invalid method pairings. The thread cases cover the different
reply settings, tagged objects, and schema-declared alternatives.
The response ingress cases exercise the typed decoder on raw JSON-RPC results,
compare opaque numbers with `Value` ingress, and pin the buffering restriction.
The pinned inputs and their provenance live in `verification/schemas/pins.json`.

Run the crate tests with `cargo nextest run --locked -p vendor-protocol`.
From the repository root, `pnpm --dir verification run codex:generate`
regenerates the committed Rust and `pnpm --dir verification run codex:check`
checks it offline.
