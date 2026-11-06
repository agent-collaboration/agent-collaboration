# owned-process-test-support proof

This unpublished library owns child output capture and panic-safe scenario finalization.
It depends on no Router crate. Callers supply commands, resources, cleanup and verification.

Phase 1 keeps all 15 existing helper cases in the codex-router-test-support library test
binary and their original modules, preserving each nextest binary/name identity. They
exercise the extracted implementation through this crate, including exact controlled-child
selection, concurrent pipes, termination and wait, real peer cleanup, and original errors
and panic payloads. The controlled child remains ignored and is invoked by its parent cases.

Run their real proof with:

```sh
cargo nextest run --locked --profile ci -p codex-router-test-support
```

This crate declares no integration or e2e target; its retained contract cases live in the
original package. No empty aggregate is added.
