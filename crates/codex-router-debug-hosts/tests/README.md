# codex-router-debug-hosts test contracts

This package declares no integration or e2e test target. Its binaries are
development-only debug Host launchers (`cargo run -p codex-router-debug-hosts
--bin <name>`); no production crate depends on it. The binaries keep
`test = false`, unchanged from their former example and bin targets.
