# Fleet copy workerd controls

This suite preserves the five rename streams, four copy-certainty controls and
22 foreign-key controls from agent-fleet-sync b9470dc. Exact case identities are
in `case-identities.json`. The worker is a separate wasm workspace at
`crates/fleet-project-copy/tests/workerd`; the seven raw-SQL cases live in the
sibling `fleet-migrations-workerd` folder. Native Turso does not replace workerd
SQLite. These local controls establish no hosted Cloudflare behavior.

From the repository root, with wasm-bindgen CLI 0.2.129 available:

```sh
WASM_BINDGEN=/absolute/path/to/wasm-bindgen node verification/tooling/run-fleet-workerd.ts /absolute/fresh/evidence-directory
```

The launcher exports fixtures through the retained signed native tests, builds
the isolated worker, generates bindings, prepares the lossless manifest, and
runs every controller. `stages.json` records each command, exit and elapsed time;
controller `results.json` files preserve exact case names. `complete.json` is
written only after all 31 adapter and seven SQL cases pass. Existing nonempty
evidence is refused. Optional supplied snapshot entries are also replayed and
verified; they have no fixed case denominator.

`prepare-matrix.ts` is a nonmechanical port of the old Python oracle. It keeps
exact wide integer lexemes, blob/null values, checkpoint positions, split-part
completion, offline streams, local-table absence checks and optional snapshots.
The permanent port tests cover those branches with literal oracles.

Current repository Miniflare/workerd pins replace the upstream runtime pins.
That is compatibility work. Miniflare 5 cannot translate the old module globs;
`worker-modules.ts` declares all five generated runtime assets, entrypoint
first, and checks actual files recursively. Added/missing files or a changed
snippet hash fail before launch. Declaration metadata is excluded. Case-set
validation rejects empty, missing, duplicate, wrong-name or prefix-only runs.
All original assertions and per-request deadlines remain.

Copy-certainty reply loss is injected after a real commit/rollback. It proves
the classifier and durable effects, not physical I/O failure. Rename streams
reopen the native writer where named; they do not claim a workerd process
restart. The optional snapshot path remains separately parameterized.
