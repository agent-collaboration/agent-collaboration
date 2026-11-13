# Debug Host test contracts

The five bin targets enable their colocated unit tests. Run them with
`cargo nextest run -p codex-router-debug-hosts --features runtime-test-support`.
They cover automation launch options, permissions and diagnostics; board Host
argument parsing; acceptance ownership and proof settings; and imagegen audit
and profile projection. The provider-only launcher has no unit cases.

There are no integration or e2e aggregates in this package. These development
launchers are never dependencies of production crates. Build the normal
automation Host without features to keep test credential guards out.
