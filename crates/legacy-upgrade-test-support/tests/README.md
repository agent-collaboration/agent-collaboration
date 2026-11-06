# Legacy importer CLI fixture contract

This unpublished library owns actual_cli: real importer invocation, guarded access
assertion files, cleanup and publication-report validation. The caller supplies the
compiled CLI, signing runner, isolated input paths and already-running central.
It uses owned-process-test-support without depending on the production importer.

The original cases remain in board-legacy-upgrade integration/e2e and
agent-collaboration e2e. Their package READMEs describe the real-process setup.
This crate declares no integration or e2e aggregate and moves no cases.
